use std::fs::File;
use std::io::{BufReader, Read};
use nbt::decode::read_zlib_compound_tag;
use std::io::Cursor;
use std::default::Default;
use std::cmp::max;
use std::time::Instant;
use lazy_static::lazy_static;
use rayon::prelude::*;


#[derive(Debug, Clone, Copy)]
struct Block<'a> {
    block_name: &'a str,
    namespace: &'a str,
    world_pos: Option<(i32, i32, i32)>,
    chunk_pos: Option<(i32, i32)>,
    properties: &'a nbt::CompoundTag,
    biome: Option<&'a str>,
    is_structure: bool,
}

lazy_static! { // this uses the lazy_static crate; `use lazy_static::lazy_static`
    static ref EMPTY_TAG: nbt::CompoundTag = nbt::CompoundTag::new();
}

impl<'a> Default for Block<'a> {
    fn default() -> Self {
        Block {
            block_name: "air",
            namespace: "minecraft",
            world_pos: None,
            chunk_pos: None,
            properties: &EMPTY_TAG,
            biome: None,
            is_structure: false,
        }
    }
}


fn get_chunk_offset(data: &Vec<u8>, chunk_x: i32, chunk_z: i32) -> (i32, u8) {
    let b_off = 4 * (chunk_x % 32 + chunk_z % 32 * 32); // V
    let b_off_usize = b_off as usize;
    let off_bytes = &data[b_off_usize..(b_off_usize + 3)];
    let off = i32::from_be_bytes([0, off_bytes[0], off_bytes[1], off_bytes[2]]);
    let sectors = data[b_off_usize + 3];
    (off, sectors)
}


fn read_chunk(data: Vec<u8>, chunk_x: i32, chunk_z: i32) -> Result<nbt::CompoundTag, ()> {
    let off = get_chunk_offset(&data, chunk_x, chunk_z);

    if off == (0, 0) {
        return Err(());
    }


    let uoff = off.0 as usize * 4096; // V
    let length_bytes = &data[uoff..uoff + 4];
    let length = i32::from_be_bytes(length_bytes.try_into().unwrap());
    let compressed_data = &data[(uoff + 5)..(uoff + 5 + (length as usize) + 1)];
    
    let mut cursor: Cursor<&[u8]> = Cursor::new(compressed_data); 
    Ok(read_zlib_compound_tag(&mut cursor).unwrap())
}

fn bit_length(num: i32) -> u32 {
    if num == 0 {
        return 1;
    }
    let abs_num = num.abs();
    32 - abs_num.leading_zeros()
}

/*

def calculate_position(block_index, chunk_x, chunk_z, min_section_value, section_index):
    section_relative_y = block_index//256
    base_calc = block_index-section_relative_y*256
    relative_chunk_x = base_calc%16
    relative_chunk_z = base_calc//16
    world_x = relative_chunk_x+(chunk_x)*16
    world_z = relative_chunk_z+(chunk_z)*16
    world_y = (section_index+min_section_value)*16 + section_relative_y
    return world_x, world_z, world_y
*/

fn calculate_position(block_index: i32, chunk_x: i32, chunk_z: i32, min_section_value: i32, section_index: i32) -> (i32, i32, i32) {
    let section_relative_y = block_index / 256;

    let base_calc = block_index - section_relative_y * 256;
    let relative_chunk_x = base_calc % 16;
    let relative_chunk_z = base_calc / 16;

    let world_x = relative_chunk_x + (chunk_x) * 16;
    let world_z = relative_chunk_z + (chunk_z) * 16;
    let world_y = (section_index + min_section_value) * 16 + section_relative_y as i32;
    (world_x as i32, world_y, world_z as i32)
}

fn parse_blocks(chunk_nbt: &nbt::CompoundTag) -> [[Block; 4096]; 25] {
    let empty_block: Block = Default::default();
    let chunk_sections_nbt = chunk_nbt.get_compound_tag_vec("sections").unwrap();
    let sections_amount = chunk_sections_nbt.len();

    let chunk_x = chunk_nbt.get_i32("xPos").unwrap();
    let chunk_z = chunk_nbt.get_i32("zPos").unwrap();

    let mut min_viable_section_ind: i32 = 0;
    let mut found_min_viable = false;
    let min_section_value = chunk_sections_nbt[0].get_i8("Y").unwrap() as i32;

    let mut section_block_states = Vec::with_capacity(sections_amount);

    for section_index in 0..sections_amount {
        if let Ok(curr_section_block_states) = chunk_sections_nbt[section_index].get_compound_tag("block_states") {
            section_block_states.push(curr_section_block_states);
            if !found_min_viable {
                min_viable_section_ind = section_index as i32;
                found_min_viable = true;
            }
        } else {
            println!("Failed to get block states for section {} #34DDE", section_index);
        }
    }

    let mut chunk: [[Block; 4096]; 25] = [[empty_block; 4096]; 25];

    section_block_states.par_iter_mut().zip(&mut chunk).enumerate().take(sections_amount).skip(min_viable_section_ind as usize).for_each(|(section_index,(block_states_list, chunk))| {

        if let (Ok(tmp_block_states_data), Ok(tmp_block_palette)) = (
            block_states_list.get_i64_vec("data"),
            block_states_list.get_compound_tag_vec("palette"),
        ) {
            let block_states_data = tmp_block_states_data;
            let block_palette = tmp_block_palette;

            let bits_per_block = max(bit_length((block_palette.len() as i32 - 1) as i32), 4) as i32;

            let mut block_index = 0;
            let mut block_states_data_index = 0;
            let mut current_long: i64 = block_states_data[block_states_data_index];

            if current_long < 0 {
                current_long += 256;
            }

            let max_index_value = (1 << bits_per_block) - 1;
            let mut long_length = 64;

            for section_block_index in 0..4096 {
                if long_length < bits_per_block {
                    block_states_data_index += 1;
                    current_long = block_states_data[block_states_data_index];

                    if current_long < 0 {
                        let tmp_long: i128 = current_long as i128 + 18446744073709551616; // 2^64
                        current_long = tmp_long as i64;
                    }

                    long_length = 64;
                }

                let current_palette_block = &block_palette[(current_long & max_index_value) as usize];
                let block_full_name = current_palette_block.get_str("Name").unwrap();

                if block_full_name == "minecraft:air" {
                    block_index += 1;
                    current_long >>= bits_per_block;
                    long_length -= bits_per_block;
                    continue;
                }

                let (block_namespace, block_name) = match block_full_name.find(':') {
                    Some(idx) => (&block_full_name[..idx], &block_full_name[(idx + 1)..]),
                    None => ("minecraft", "air"),
                };
                let world_pos = calculate_position(block_index, chunk_x, chunk_z, min_section_value, section_index as i32);

                let properties = current_palette_block.get_compound_tag("Properties");
                let new_props = if let Ok(new_props) = properties { new_props } else { &EMPTY_TAG };
                block_index += 1;
                current_long >>= bits_per_block;
                long_length -= bits_per_block;

                chunk[section_block_index] = Block {
                    block_name: block_name,
                    namespace: block_namespace,
                    world_pos: Some(world_pos),
                    properties: new_props,
                    ..Default::default()
                };
            }
        }
    });

    chunk
}

fn main() {
    
    // Handle the file opening operation properly
    let file_result = File::open("D:\\ServersTest\\Test2\\world\\region\\r.-3.-2.mca");
    match file_result {
        Ok(f) => {
            let mut reader = BufReader::new(f);
            let mut buffer = Vec::new();

            // Read file into vector.
            if reader.read_to_end(&mut buffer).is_ok() {
                let now = Instant::now();
                let cnk_nbt = read_chunk(buffer, 15, 30).unwrap();
                let data = parse_blocks(&cnk_nbt);
                let s = now.elapsed();
                println!("Time: {:?}", s);
            } else {
                eprintln!("Failed to read the file.");
            }
        }
        Err(err) => {
            eprintln!("Failed to open the file: {:?}", err);
        }
    }
}


