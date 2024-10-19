import os
import re


def add_quotes(text):
    # Regular expression to find a word followed by a colon
    pattern = r'(\b\w+):'

    
    # Use re.sub to replace the matches with quoted versions
    result = re.sub(pattern, r'"\1":', text)
    
    return result


def balance_json_brackets(json_string: str) -> str:
    stack = []  # To track opening brackets and their positions
    result = list(json_string)  # Work with the string as a list for easier manipulation
    insertions = []  # To keep track of where we need to insert opening brackets

    open_ones = 0
    # First pass: Detect and record mismatched brackets
    
    result = ''
    unmatched = 0
    next_closing = 0
    grow = 0
    for i, char in enumerate(json_string):
        if char in '{' and open_ones == 2 and json_string[i:].find('}') < json_string[i:].find('{') : 
            grow += 1
            next_closing = json_string[i:].find('}')
            result = result + json_string[:next_closing] + '}'
            open_ones = 0
        elif char in '{':
             open_ones = 0 if open_ones > 2 else open_ones + 1
    result = result + json_string[next_closing:]
    return result     

def process_file_content(content):
    # Define the patterns to remove (including quotation marks and the colon):
    remove_names = [
        'EncodedConfirmedTransactionWithStatusMeta',
        'EncodedTransactionWithStatusMeta',
        'UiTransactionStatusMeta',
        'TransactionError',
        'TransactionResult',
        'UiInnerInstructions',
        'UiTransactionTokenBalance',
        'UiTokenAmount',
        'Reward',
        'Rewards',
        'RewardType',
        'UiLoadedAddresses',
        'UiTransactionReturnData',
        'UiReturnDataEncoding',
        'UiTransaction',
        'Parsed(UiParsedMessage'
        'ParsedAccount',
        'UiAddressTableLookup',
        'UiPartiallyDecodedInstruction',
        'ParsedInstruction',
        'UiCompiledInstruction',
        'Object',
    ]
    
    # # Remove specified names
    for name in remove_names:
        content = content.replace(f'{name}', '')

    # # # Replace `None` with `null`
    content = content.replace('None', 'null')

    # # # Put quotation marks around all occurrences of Some(Transaction)
    content = re.sub(r'Some\(Transaction\)', '"Some(Transaction)"', content)

    content = content.replace("Parsed(", "Parsed: {").replace('Compiled(', 'Compiled: {').replace('PartiallyDecoded(', 'PartiallyDecoded: {')

    content = content.replace("{ {", "{")

    # # Balance the brackets 
    content = balance_json_brackets(content)
    # # Replace program_id
    content = content.replace('CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C', '6xPaJUuGmeTS19NJrya76jRNQSnxmH1vCj8SMvLutKwy')
    
    # # content = content.replace()
    content = add_quotes(content)


    # # Remove occurrences of specific wrappers
    wrappers_to_remove = [
        'Json(', 
        # 'Parsed(', 
        # 'PartiallyDecoded(',
        # 'Compiled(',
        'Some(',
        'String(',
        'Compiled(',
        'Number('
    ]
    for wrapper in wrappers_to_remove:
        content = content.replace(wrapper, '')

    # # Function to remove unmatched parentheses
    def remove_unmatched_parentheses(s):
        result = []
        open_parentheses = 0

        for char in s:
            if char == '(':
                open_parentheses += 1
                result.append(char)
            elif char == ')':
                if open_parentheses > 0:
                    open_parentheses -= 1
                    result.append(char)
            else:
                result.append(char)

        return ''.join(result)
    
    def remove_unmatched_closing_parantheses(s):
        result = []
        open_parentheses = 0

        for char in reversed(s):
            if char == ')':
                open_parentheses += 1
                result.append(char)
            elif char == '(':
                if open_parentheses > 0:
                    open_parentheses -= 1
                    result.append(char)
            else:
                result.append(char)
        
        return ''.join(reversed(result))

        
    
    def find_all_ending_indices(input_str, starting_indices, char_0, char_1):
        closing_indices = []
        for start in starting_indices:
            stack = 1  # Counter to keep track of open brackets
            # Start searching from the position after the open bracket
            for i in range(start + 1, len(input_str)):
                if input_str[i] == char_0:
                    stack += 1  # Increment stack for each open bracket
                elif input_str[i] == char_1:
                    stack -= 1  # Decrement stack for each closing bracket
                # If stack reaches 0, it means we found the matching closing bracket
                if stack == 0:
                    closing_indices.append(i)
                    break
                
        
        return closing_indices # Return -1 if no matching closing bracket is found

    def find_all_occurrences(main_string, substring):
        start = 0
        indices = []
        while True:
            start = main_string.find(substring, start)
            if start == -1:  
                break
            indices.append(start + len(substring))
            start += len(substring)
        return indices

    
    def remove_substrings(original_str, start_indices, end_indices):
        if len(start_indices) != len(end_indices):
            raise ValueError("Start and end indices lists must have the same length.")

        # Convert string to a list to easily manipulate it
        char_list = list(original_str)
        
        # Iterate over start and end indices in reverse order to avoid index shifting issues
        for start, end in sorted(zip(start_indices, end_indices), reverse=True):
            # Remove the substring from start to end (inclusive)
            del char_list[start+1:end]

        # Join the list back into a string
        return ''.join(char_list)
    
    def remove_substrings_return_data(original_str, start_indices, end_indices):
        if len(start_indices) != len(end_indices):
            raise ValueError("Start and end indices lists must have the same length.")

        # Convert string to a list to easily manipulate it
        char_list = list(original_str)
        
        # Iterate over start and end indices in reverse order to avoid index shifting issues
        for start, end in sorted(zip(start_indices, end_indices), reverse=True):
            # Remove the substring from start to end (inclusive)
            char_list[start:end+1] = list("null") 

        # Join the list back into a string
        return ''.join(char_list)

    

    # # Remove unmatched parentheses from content
    content = remove_unmatched_parentheses(content)

    content = content.replace("\"Parsed\": {UiParsedMessage", "")
    content = content.replace("ParsedAccount", "")
    content = content.replace("instructions\": [\"Parsed\"", "instructions\": [{\"Parsed\"")
    content = content.replace(" \"Parsed\": {\"PartiallyDecoded\"", "{\"Parsed\": {\"PartiallyDecoded\"")
    content = content.replace(" \"Parsed\": {\"Parsed\":", "{\"Parsed\": {\"Parsed\":")
    content = content.replace("\"stack_height\": null }", "\"stack_height\": null }}}")
    content = content.replace("\"stack_height\": 2 }", "\"stack_height\": 2 }}}")
    content = content.replace("\"stack_height\": 3 }", "\"stack_height\": 3 }}}")
    content = content.replace("\"stack_height\": 4 }", "\"stack_height\": 4 }}}")
    content = content.replace("LookupTable", "\"LookupTable\"")
    content = content.replace("\"err\": null, \"status\": Ok(()),", "")
    content = content.replace("Array", "")
    content = balance_json_brackets(content)
    content = content.replace("Program \"log\"", "Program log")
    content = content.replace("Skip", "null")
    starting_indices = find_all_occurrences(content, "\"log_messages\": ")
    ending_indices = find_all_ending_indices(content, starting_indices, '[', ']')
    result = remove_substrings(content, starting_indices, ending_indices)

    starting_indices = find_all_occurrences(result, "\"return_data\":  ")
    ending_indices = find_all_ending_indices(result, starting_indices, '{', '}')
    result = remove_substrings_return_data(result, starting_indices, ending_indices)
    # result = remove_unmatched_closing_parantheses(result)

    
    return result

def process_files_in_folder(folder_path):
    # Iterate through all files in the folder
    for root, dirs, files in os.walk(folder_path):
        for file in files:
            file_path = os.path.join(root, file)
            # Only process text-based files
            if file.endswith('.txt') or file.endswith('.json'):  # Adjust extensions if needed
                with open(file_path, 'r') as f:
                    content = f.read()
                
                # Process the file content
                new_content = process_file_content(content)
                
                # Write the processed content back to the file
                with open(file_path, 'w') as f:
                    f.write(new_content)
                print(f'Processed: {file_path}')


if __name__ == "__main__":
    folder_path = "/Users/jackfisher/Desktop/new-audits/raydium-cp-swap/fomo3d-raydium-cp-swap-client/cp-swap-txs"
    process_files_in_folder(folder_path)
