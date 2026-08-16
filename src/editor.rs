use std::env;
use std::fs;
use std::io;
use std::process::Command;

pub fn edit_content(editor_cmd: &str, initial: &str) -> io::Result<String> {
    let tmp = env::temp_dir().join("console_note_edit.md");
    fs::write(&tmp, initial)?;

    let parts: Vec<&str> = editor_cmd.split_whitespace().collect();
    let status = Command::new(parts[0])
        .args(&parts[1..])
        .arg(&tmp)
        .status()?;

    if !status.success() {
        return Err(io::Error::other("editor exited with error"));
    }

    let result = fs::read_to_string(&tmp)?;
    let _ = fs::remove_file(&tmp);
    Ok(result)
}
