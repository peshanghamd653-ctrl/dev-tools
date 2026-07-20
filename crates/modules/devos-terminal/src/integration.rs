//! Shell integration: OSC 133 command-completion markers.
//!
//! PowerShell sessions get a prompt hook (injected via `-EncodedCommand`,
//! chaining the user's existing prompt so Starship etc. keep working) that
//! writes `ESC ] 133 ; D ; <exit-code> BEL` before every prompt. The pty
//! reader scans the output stream for these markers; non-zero codes become
//! `CommandFailure` events the desktop turns into notifications.

use base64::Engine;

/// A command finished with a non-zero exit code in some session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandFailure {
    pub session_id: String,
    pub exit_code: i64,
}

/// Is this shell one we know how to inject the prompt hook into?
pub fn supports_integration(shell: &str) -> bool {
    matches!(
        std::path::Path::new(shell)
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .as_deref(),
        Some("powershell") | Some("pwsh")
    )
}

/// The prompt hook, base64(UTF-16LE)-encoded for `-EncodedCommand` — no
/// quoting hell, works identically for powershell.exe and pwsh.
pub fn encoded_prompt_hook() -> String {
    const SCRIPT: &str = r#"
$global:__devosPrompt = $function:prompt
function global:prompt {
  $q = $?
  $code = 0
  if (-not $q) {
    if ($global:LASTEXITCODE -is [int] -and $global:LASTEXITCODE -ne 0) { $code = $global:LASTEXITCODE }
    else { $code = 1 }
  }
  $e = [char]27; $b = [char]7
  [Console]::Write("$e]133;D;$code$b")
  & $global:__devosPrompt
}
"#;
    let utf16: Vec<u8> = SCRIPT
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

const PREFIX: &[u8] = b"\x1b]133;D;";
const MAX_CODE_DIGITS: usize = 12;

/// Cross-chunk scanner for OSC 133;D markers. Feed raw pty bytes in any
/// split; complete markers yield their exit codes (including 0 — callers
/// filter). Incomplete trailing sequences are carried to the next feed.
#[derive(Default)]
pub struct OscScanner {
    carry: Vec<u8>,
}

impl OscScanner {
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<i64> {
        self.carry.extend_from_slice(chunk);
        let data = std::mem::take(&mut self.carry);
        let mut codes = Vec::new();
        let mut pos = 0;
        let mut resume: Option<usize> = None;

        'scan: while pos < data.len() {
            let Some(esc_offset) = data[pos..].iter().position(|&b| b == 0x1b) else {
                break;
            };
            let start = pos + esc_offset;
            let available = &data[start..];

            if available.len() < PREFIX.len() {
                if PREFIX.starts_with(available) {
                    resume = Some(start);
                }
                break;
            }
            if &available[..PREFIX.len()] != PREFIX {
                pos = start + 1;
                continue;
            }

            // Parse the exit code until BEL or ESC (start of ST).
            let mut j = PREFIX.len();
            loop {
                if j >= available.len() {
                    // Marker not terminated yet — wait for more bytes,
                    // unless it has grown past any sane exit code.
                    if j - PREFIX.len() <= MAX_CODE_DIGITS {
                        resume = Some(start);
                    }
                    break 'scan;
                }
                let byte = available[j];
                if byte == 0x07 || byte == 0x1b {
                    if let Ok(text) = std::str::from_utf8(&available[PREFIX.len()..j]) {
                        if let Ok(code) = text.parse::<i64>() {
                            codes.push(code);
                        }
                    }
                    pos = start + j;
                    continue 'scan;
                }
                if j - PREFIX.len() > MAX_CODE_DIGITS {
                    pos = start + j;
                    continue 'scan;
                }
                j += 1;
            }
        }

        let keep_from = resume.unwrap_or_else(|| data.len().saturating_sub(PREFIX.len() - 1));
        self.carry = data[keep_from..].to_vec();
        codes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_codes_across_every_possible_split() {
        let payload = b"before\x1b]133;D;42\x07middle\x1b]133;D;0\x07after";
        for split in 0..=payload.len() {
            let mut scanner = OscScanner::default();
            let mut codes = scanner.feed(&payload[..split]);
            codes.extend(scanner.feed(&payload[split..]));
            assert_eq!(codes, vec![42, 0], "split at {split}");
        }
    }

    #[test]
    fn handles_st_terminator_negative_codes_and_garbage() {
        let mut scanner = OscScanner::default();
        assert_eq!(scanner.feed(b"\x1b]133;D;7\x1b\\rest"), vec![7]);
        assert_eq!(scanner.feed(b"\x1b]133;D;-1\x07"), vec![-1]);
        assert_eq!(
            scanner.feed(b"\x1b]133;D;notanumber\x07"),
            Vec::<i64>::new()
        );
        // Unterminated garbage past the digit budget is abandoned.
        assert_eq!(
            scanner.feed(b"\x1b]133;D;12345678901234567890"),
            Vec::<i64>::new()
        );
        // Other OSC/CSI traffic is ignored.
        assert_eq!(
            scanner.feed(b"\x1b]0;title\x07\x1b[32mgreen\x1b[0m"),
            Vec::<i64>::new()
        );
    }

    #[test]
    fn powershell_detection() {
        assert!(supports_integration("powershell.exe"));
        assert!(supports_integration(
            "C:/Program Files/PowerShell/7/pwsh.exe"
        ));
        assert!(!supports_integration("cmd.exe"));
        assert!(!supports_integration("/bin/bash"));
    }

    #[test]
    fn encoded_hook_is_valid_base64_utf16() {
        let encoded = encoded_prompt_hook();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .expect("valid base64");
        assert_eq!(bytes.len() % 2, 0, "UTF-16 byte length must be even");
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        let script = String::from_utf16(&units).expect("valid UTF-16");
        assert!(script.contains("133;D;"));
        assert!(script.contains("__devosPrompt"));
    }
}
