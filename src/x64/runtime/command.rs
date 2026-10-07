//! Shell command-line quoting, including the special executable argument.
pub(super) fn arguments(command: &str) -> Vec<String> {
    let input: Vec<char> = command.chars().collect();
    let mut at = 0;
    let mut args = Vec::new();
    let mut first = String::new();
    if input.first() == Some(&'"') {
        at = 1;
        while at < input.len() && input[at] != '"' {
            first.push(input[at]);
            at += 1;
        }
        if at < input.len() {
            at += 1;
        }
    } else {
        while at < input.len() && !matches!(input[at], ' ' | '\t') {
            first.push(input[at]);
            at += 1;
        }
    }
    args.push(first);
    loop {
        while at < input.len() && matches!(input[at], ' ' | '\t') {
            at += 1;
        }
        if at == input.len() {
            break;
        }
        let mut value = String::new();
        let mut quotes = 0;
        while at < input.len() && !(quotes == 0 && matches!(input[at], ' ' | '\t')) {
            let mut slashes = 0;
            while at < input.len() && input[at] == '\\' {
                slashes += 1;
                at += 1;
            }
            if at < input.len() && input[at] == '"' {
                value.extend(std::iter::repeat_n('\\', slashes / 2));
                if slashes % 2 != 0 {
                    value.push('"');
                } else {
                    quotes += 1;
                }
                at += 1;
                while at < input.len() && input[at] == '"' {
                    quotes += 1;
                    if quotes == 3 {
                        value.push('"');
                        quotes = 0;
                    }
                    at += 1;
                }
                if quotes == 2 {
                    quotes = 0;
                }
            } else {
                value.extend(std::iter::repeat_n('\\', slashes));
                if at < input.len() && !(quotes == 0 && matches!(input[at], ' ' | '\t')) {
                    value.push(input[at]);
                    at += 1;
                }
            }
        }
        args.push(value);
    }
    args
}
