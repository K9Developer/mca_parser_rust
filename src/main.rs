fn parse_blocks(chunk_nbt: &nbt::CompoundTag) -> Vec<[Block<'_>; 4096]> {
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

    let mut chunk = vec![[empty_block; 4096];25];
    
    for section_index in min_viable_section_ind..sections_amount as i32 {
        let mut hash_block_palette: HashMap<usize, [String; 2]> = HashMap::new();
        let block_states_list = &section_block_states[section_index as usize];

        if let (Ok(tmp_block_states_data), Ok(tmp_block_palette)) = (
            block_states_list.get_i64_vec("data"),
            block_states_list.get_compound_tag_vec("palette"),
        ) {
            let block_states_data = tmp_block_states_data;
            let block_palette = tmp_block_palette;

            block_palette.to_vec().iter().enumerate().for_each(|(ind, block)| {
                hash_block_palette.insert(ind, [block.get_str("Name").unwrap().to_string(), block.get_str("Properties").unwrap().to_string()]);
            });

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
                        let tmp_long: i64 = current_long as i64 + 0; // 2^64
                        current_long = tmp_long as i64;
                    }

                    long_length = 64;
                }

                let current_palette_block = hash_block_palette[&((current_long & max_index_value) as usize)].clone();
                let block_full_name = current_palette_block[0].clone();

                if block_full_name == "minecraft:air" {
                    block_index += 1;
                    current_long >>= bits_per_block;
                    long_length -= bits_per_block;
                    continue;
                }

                let cloned = block_full_name.clone();
                let (block_namespace, block_name) = block_full_name.split_once(':').unwrap_or(("minecraft", "air"));
                let world_pos = calculate_position(block_index, chunk_x, chunk_z, min_section_value, section_index as i32);

                let properties = &(current_palette_block[1].clone());
                // let new_props = if let Ok(new_props) = properties { new_props } else { &EMPTY_TAG };
                block_index += 1;
                current_long >>= bits_per_block;
                long_length -= bits_per_block;

                chunk[section_index as usize][section_block_index] = Block {
                    block_name: block_name,
                    namespace: block_namespace,
                    world_pos: Some(world_pos),
                    properties: properties,
                    ..Default::default()
                };
            }
        }
    }

    chunk
}
