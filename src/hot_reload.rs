use std::collections::HashSet;
use std::fs;
use std::rc::Rc;
use std::time::SystemTime;

use crate::compiler::Compiler;
use crate::error::Error;
use crate::lexer::Lexer;
use crate::parser;
use crate::value::Value;
use crate::vm::VirtualMachine;

pub struct HotReloadEngine {
    pub vm: VirtualMachine,
    script_path: String,
    last_modified: Option<SystemTime>,
}

impl HotReloadEngine {
    pub fn new(script_path: &str) -> Self {
        let mut vm = VirtualMachine::new();
        vm.register_fn("Log", |args| {
            let msg: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            println!("{}", msg.join(" "));
            Value::Nil
        });
        Self {
            vm,
            script_path: script_path.to_string(),
            last_modified: None,
        }
    }

    pub fn reload_if_changed(&mut self) -> Result<bool, Error> {
        let metadata = fs::metadata(&self.script_path)
            .map_err(|e| Error::runtime(format!("cannot stat '{}': {}", self.script_path, e)))?;
        let modified = metadata.modified().map_err(|e| {
            Error::runtime(format!(
                "cannot read modification time of '{}': {}",
                self.script_path, e
            ))
        })?;

        if self.last_modified.is_none() || self.last_modified.unwrap() < modified {
            println!("[hot reload] recompiling '{}' ...", self.script_path);
            self.last_modified = Some(modified);
            self.compile_and_swap()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn compile_and_swap(&mut self) -> Result<(), Error> {
        let source = fs::read_to_string(&self.script_path).map_err(|e| {
            Error::runtime(format!("cannot read '{}': {}", self.script_path, e))
        })?;

        let mut lexer = Lexer::new(&source);
        let tokens = lexer.tokenize()?;
        let ast = parser::parse(tokens)?;

        // Preserve live global values across code swaps: recompiling the top
        // level must not reset variables that already exist in the VM.
        let preserve: HashSet<String> = self.vm.globals.keys().cloned().collect();
        let compiler = Compiler::with_preserved_globals("main", preserve);
        let chunk = Rc::new(compiler.compile(&ast));

        self.vm.execute(chunk)
    }
}