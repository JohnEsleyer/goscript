use std::collections::{HashMap, HashSet};
use std::fs;
use std::rc::Rc;
use std::time::SystemTime;

use crate::compiler::Compiler;
use crate::error::Error;
use crate::lexer::Lexer;
use crate::parser;
use crate::resolver::resolve_imports_with_deps;
use crate::value::Value;
use crate::vm::VirtualMachine;

pub struct HotReloadEngine {
    pub vm: VirtualMachine,
    script_path: String,
    last_modified: Option<SystemTime>,
    /// Modification times of all imported dependency files, keyed by path.
    dep_modified: HashMap<String, SystemTime>,
    /// All dependency file paths discovered during the last compile.
    dep_paths: HashSet<String>,
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
            dep_modified: HashMap::new(),
            dep_paths: HashSet::new(),
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

        let mut changed = self.last_modified.is_none() || self.last_modified.unwrap() < modified;

        // Also check if any imported dependency changed.
        if !changed {
            for dep_path in &self.dep_paths {
                if let Ok(meta) = fs::metadata(dep_path) {
                    if let Ok(mtime) = meta.modified() {
                        if self.dep_modified.get(dep_path).map(|t| *t) < Some(mtime) {
                            changed = true;
                            break;
                        }
                    }
                }
            }
        }

        if changed {
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
        // Resolve `import`ed modules and collect all dependency paths so we
        // can watch them for changes on the next tick.
        let (ast, dep_paths) =
            resolve_imports_with_deps(ast, self.vm.resolver.as_ref())?;

        // Update dep tracking: record modification times for every dependency.
        let mut dep_modified = HashMap::new();
        for dep in &dep_paths {
            if let Ok(meta) = fs::metadata(dep) {
                if let Ok(mtime) = meta.modified() {
                    dep_modified.insert(dep.clone(), mtime);
                }
            }
        }
        self.dep_paths = dep_paths;
        self.dep_modified = dep_modified;

        // Preserve live global values across code swaps: recompiling the top
        // level must not reset variables that already exist in the VM.
        let preserve: HashSet<String> = self.vm.globals.keys().cloned().collect();
        let compiler = Compiler::with_preserved_globals("main", preserve);
        let chunk = Rc::new(compiler.compile(&ast));

        self.vm.execute(chunk)
    }
}
