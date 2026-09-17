use std::fs::{self,File,OpenOptions}; use std::io::{BufReader,BufRead,Read,Seek,SeekFrom,Write}; use std::path::Path;
fn display_error(e: impl std::fmt::Display)->String {e.to_string()}
fn project_journal_range(
    journal: &Path,
    offset: u64,
    length: u64,
    include_agent_updates: bool,
    output: &mut File,
) -> Result<(), String> {
    let mut reader = BufReader::new(File::open(journal).map_err(display_error)?);
    reader
        .seek(SeekFrom::Start(offset))
        .map_err(display_error)?;
    for line in reader
        .take(length.saturating_sub(offset))
        .lines()
        .map_while(Result::ok)
    {
        if !line.contains(r#""source":"agent""#)
            || (include_agent_updates && is_visible_agent_line(&line))
        {
            output.write_all(line.as_bytes()).map_err(display_error)?;
            output.write_all(b"\n").map_err(display_error)?;
        }
    }
    Ok(())
}

fn is_visible_agent_line(line: &str) -> bool {
    line.contains(r#""type":"agent_message""#) || line.contains(r#""type":"experience_updated""#)
}


fn main(){
let path=Path::new("/tmp/3720-live-audit-partial.jsonl");let out=Path::new("/tmp/3720-live-audit-projected.jsonl");
let first=r#"{"source":"game","sequence":1,"payload":"#;
fs::write(path,first).unwrap();let mut dest=File::create(out).unwrap();project_journal_range(path,0,first.len() as u64,true,&mut dest).unwrap();
let mut file=OpenOptions::new().append(true).open(path).unwrap();file.write_all(b"42}\n").unwrap();
project_journal_range(path,first.len() as u64,fs::metadata(path).unwrap().len(),true,&mut dest).unwrap();
println!("authority={}projection={}",fs::read_to_string(path).unwrap(),fs::read_to_string(out).unwrap());
}
