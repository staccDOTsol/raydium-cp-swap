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
    content.replace('CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C', '6xPaJUuGmeTS19NJrya76jRNQSnxmH1vCj8SMvLutKwy')
    
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
    
    def find_closing_bracket(input_str, open_pos):
        if input_str[open_pos] != '[':
            return -1  # Return -1 if the character at open_pos is not an open bracket
        
        stack = 0  # Counter to keep track of open brackets
        
        # Start searching from the position after the open bracket
        for i in range(open_pos, len(input_str)):
            if input_str[i] == '[':
                stack += 1  # Increment stack for each open bracket
            elif input_str[i] == ']':
                stack -= 1  # Decrement stack for each closing bracket
                
            # If stack reaches 0, it means we found the matching closing bracket
            if stack == 0:
                return i
        
        return -1  # Return -1 if no matching closing bracket is found

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
    starting_indices = find_all_occurrences(content, "\"log_messages\": ")

    result = ""
    for start_index in starting_indices:
        closing_index = find_closing_bracket(content, start_index)
        result += content[:start_index] + "[" + content[closing_index:]

    result = result.replace("Skip", "null")
    
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
