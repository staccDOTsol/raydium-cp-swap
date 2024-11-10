use regex::Regex;
use rayon::prelude::*;
use std::fs;
use std::fs::read_dir;
use std::io::{Read, Write};
use std::path::Path;

// Function to add quotes around any word followed by a colon
fn add_quotes(text: &str) -> String {
    let re = Regex::new(r"(\b\w+):").unwrap();
    re.replace_all(text, r#""$1":"#).into_owned()
}

fn remove_unmatched_parentheses(s: &str) -> String {
    let mut result = String::new();
    let mut open_parentheses = 0;

    for char in s.chars() {
        match char {
            '(' => {
                open_parentheses += 1;
                result.push(char);
            }
            ')' => {
                if open_parentheses > 0 {
                    open_parentheses -= 1;
                    result.push(char);
                }
            }
            _ => result.push(char),
        }
    }
    result
}

fn remove_unmatched_closing_parentheses(s: &str) -> String {
    let mut result = String::new();
    let mut open_parentheses = 0;

    for char in s.chars().rev() {
        match char {
            ')' => {
                open_parentheses += 1;
                result.push(char);
            }
            '(' => {
                if open_parentheses > 0 {
                    open_parentheses -= 1;
                    result.push(char);
                }
            }
            _ => result.push(char),
        }
    }
    result.chars().rev().collect()
}

fn find_all_ending_indices(input_str: &str, starting_indices: &[usize], char_0: char, char_1: char) -> Vec<usize> {
    let mut closing_indices = Vec::new();

    for &start in starting_indices {
        let mut stack = 1;
        for (i, c) in input_str.chars().enumerate().skip(start + 1) {
            if c == char_0 {
                stack += 1;
            } else if c == char_1 {
                stack -= 1;
            }
            if stack == 0 {
                closing_indices.push(i);
                break;
            }
        }
    }
    closing_indices
}

fn find_all_occurrences(main_string: &str, substring: &str) -> Vec<usize> {
    let mut indices = Vec::new();
    let mut start = 0;

    while let Some(pos) = main_string[start..].find(substring) {
        indices.push(start + pos + substring.len());
        start += pos + substring.len();
    }
    indices
}

fn remove_substrings(original_str: &str, start_indices: &[usize], end_indices: &[usize]) -> Result<String, &'static str> {
    if start_indices.len() != end_indices.len() {
        return Err("Start and end indices lists must have the same length.");
    }

    let mut char_list: Vec<char> = original_str.chars().collect();
    let mut pairs: Vec<(usize, usize)> = start_indices.iter().zip(end_indices.iter()).map(|(&s, &e)| (s + 1, e)).collect();
    pairs.sort_by(|a, b| b.cmp(a));

    for (start, end) in pairs {
        char_list.drain(start..=end);
    }

    Ok(char_list.iter().collect())
}

fn remove_substrings_return_data(original_str: &str, start_indices: &[usize], end_indices: &[usize]) -> Result<String, &'static str> {
    if start_indices.len() != end_indices.len() {
        return Err("Start and end indices lists must have the same length.");
    }

    let mut char_list: Vec<char> = original_str.chars().collect();
    let mut pairs: Vec<(usize, usize)> = start_indices.iter().zip(end_indices.iter()).map(|(&s, &e)| (s, e)).collect();
    pairs.sort_by(|a, b| b.cmp(a));

    for (start, end) in pairs {
        char_list.splice(start..=end, "null".chars());
    }

    Ok(char_list.iter().collect())
}

// Function to balance JSON brackets
fn balance_json_brackets(json_string: &str) -> String {
    let mut result = String::new();
    let mut open_brackets = 0;
    
    for c in json_string.chars() {
        match c {
            '{' => {
                open_brackets += 1;
                result.push(c);
            }
            '}' if open_brackets > 0 => {
                open_brackets -= 1;
                result.push(c);
            }
            _ => result.push(c),
        }
    }

    // Append unmatched closing brackets
    result + &"}".repeat(open_brackets)
}

// Remove specific wrappers and replace patterns
fn process_file_content(mut content: String) -> String {
    let remove_names = [
       "EncodedConfirmedTransactionWithStatusMeta",
        "EncodedTransactionWithStatusMeta",
        "UiTransactionStatusMeta",
        "TransactionError",
        "TransactionResult",
        "UiInnerInstructions",
        "UiTransactionTokenBalance",
        "UiTokenAmount",
        "Reward",
        "Rewards",
        "RewardType",
        "UiLoadedAddresses",
        "UiTransactionReturnData",
        "UiReturnDataEncoding",
        "UiTransaction",
        "Parsed(UiParsedMessage",
        "ParsedAccount",
        "UiAddressTableLookup",
        "UiPartiallyDecodedInstruction",
        "ParsedInstruction",
        "UiCompiledInstruction",
        "Object"
    ];

    for name in remove_names.iter() {
        content = content.replace(name, "");
    }
    
    content = content.replace("None", "null");
    content = content.replace("Some(Transaction)", "\"Some(Transaction)\"");
    content = content.replace("Parsed(", "Parsed: {")
                     .replace("Compiled(", "Compiled: {")
                     .replace("PartiallyDecoded(", "PartiallyDecoded: {");
    content = content.replace("{ {", "{");
    content = balance_json_brackets(&content);
    content = content.replace('CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C', '6xPaJUuGmeTS19NJrya76jRNQSnxmH1vCj8SMvLutKwy');
    content = add_quotes(&content);

    let wrappers_to_remove = ["Json(", "Some(", "String(", "Compiled(", "Number("];
    for wrapper in wrappers_to_remove.iter() {
        content = content.replace(wrapper, "");
    }
    
    content = remove_unmatched_parentheses(content);
    content = content.replace("\"Parsed\": {UiParsedMessage", "");
    content = content.replace("ParsedAccount", "");
    content = content.replace("instructions\": [\"Parsed\"", "instructions\": [{\"Parsed\"");
    content = content.replace(" \"Parsed\": {\"PartiallyDecoded\"", "{\"Parsed\": {\"PartiallyDecoded\"");
    content = content.replace(" \"Parsed\": {\"Parsed\":", "{\"Parsed\": {\"Parsed\":");
    content = content.replace("\"stack_height\": null }", "\"stack_height\": null }}}");
    content = content.replace("\"stack_height\": 2 }", "\"stack_height\": 2 }}}");
    content = content.replace("\"stack_height\": 3 }", "\"stack_height\": 3 }}}");
    content = content.replace("\"stack_height\": 4 }", "\"stack_height\": 4 }}}");
    content = content.replace("LookupTable", "\"LookupTable\"");
    content = content.replace("\"err\": null, \"status\": Ok(()),", "");
    content = content.replace("Array", "");
    content = balance_json_brackets(content);
    content = content.replace("Program \"log\"", "Program log");
    content = content.replace("Skip", "null");
    starting_indices = find_all_occurrences(content, "\"log_messages\": ");
    ending_indices = find_all_ending_indices(content, starting_indices, '[', ']');
    result = remove_substrings(content, starting_indices, ending_indices);

    starting_indices = find_all_occurrences(result, "\"return_data\":  ");
    ending_indices = find_all_ending_indices(result, starting_indices, '{', '}');
    result = remove_substrings_return_data(result, starting_indices, ending_indices);

    result
}

// Process each file's content in parallel
fn process_files_in_folder(folder_path: &str) {
    let paths: Vec<_> = read_dir(folder_path)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .collect();

    paths.par_iter().for_each(|entry| {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "txt" || ext == "json") {
            process_file(&path);
        }
    });
}

// Function to read, process, and write the content back to the file
fn process_file(path: &Path) {
    if let Ok(mut file) = fs::File::open(path) {
        let mut content = String::new();
        file.read_to_string(&mut content).unwrap();
        
        let new_content = process_file_content(content);
        
        fs::write(path, new_content).expect("Unable to write file");
    }
}

fn main() {
    let folder_path = "/home/ec2-user/raydium-cp-swap/fomo3d-raydium-cp-swap-client/big-txs";
    process_files_in_folder(folder_path);
}
