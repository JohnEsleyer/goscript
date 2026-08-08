use goscript::*;
fn main() {
    let src = std::fs::read_to_string("examples/player.gs").unwrap();
    let mut lx = lexer::Lexer::new(&src);
    match lx.tokenize() {
        Ok(toks) => {
            for t in &toks {
                println!("{}:{} {:?}", t.line, t.col, t.kind);
            }
        }
        Err(e) => println!("ERR {e:?}"),
    }
}
