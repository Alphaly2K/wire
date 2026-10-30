//! Pure Windows command-line parsing, including the special executable argument.
pub(crate) fn arguments(command: &str) -> Vec<String> {
    arguments_wide(&command.encode_utf16().collect::<Vec<_>>())
        .into_iter()
        .map(|argument| String::from_utf16(&argument).expect("UTF-16 from a Rust string"))
        .collect()
}

pub(crate) fn arguments_wide(input: &[u16]) -> Vec<Vec<u16>> {
    let mut at = 0;
    let mut args = Vec::new();
    let mut first = Vec::new();
    if input.first() == Some(&34) {
        at = 1;
        while at < input.len() && input[at] != 34 {
            first.push(input[at]);
            at += 1;
        }
        if at < input.len() {
            at += 1;
        }
    } else {
        while at < input.len() && !matches!(input[at], 32 | 9) {
            first.push(input[at]);
            at += 1;
        }
    }
    args.push(first);
    loop {
        while at < input.len() && matches!(input[at], 32 | 9) {
            at += 1;
        }
        if at == input.len() {
            break;
        }
        let mut value = Vec::new();
        let mut quotes = 0;
        while at < input.len() && !(quotes == 0 && matches!(input[at], 32 | 9)) {
            let mut slashes = 0;
            while at < input.len() && input[at] == 92 {
                slashes += 1;
                at += 1;
            }
            if at < input.len() && input[at] == 34 {
                value.extend(std::iter::repeat_n(92, slashes / 2));
                if slashes % 2 != 0 {
                    value.push(34);
                } else {
                    quotes += 1;
                }
                at += 1;
                while at < input.len() && input[at] == 34 {
                    quotes += 1;
                    if quotes == 3 {
                        value.push(34);
                        quotes = 0;
                    }
                    at += 1;
                }
                if quotes == 2 {
                    quotes = 0;
                }
            } else {
                value.extend(std::iter::repeat_n(92, slashes));
                if at < input.len() && !(quotes == 0 && matches!(input[at], 32 | 9)) {
                    value.push(input[at]);
                    at += 1;
                }
            }
        }
        args.push(value);
    }
    args
}
