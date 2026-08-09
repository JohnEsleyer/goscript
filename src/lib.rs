//! GoScript: a tiny Go-like scripting language.
//!
//! v2 architecture: source -> lexer -> parser (AST) -> bytecode compiler ->
//! register/stack VM. The crate is a library that a host game (or CLI) embeds.
//!
//! The two main entry points:
//! - [`VirtualMachine`]: compile a chunk from source and execute / call
//!   exported functions.
//! - [`HotReloadEngine`]: recompiles a script file when it changes, preserving
//!   live global state across reloads.
//!
//! Robustness features:
//! - anti-freeze instruction guard ([`VirtualMachine::set_max_instructions`])
//! - runtime errors carry script source lines (P1 source mapping)
//! - `range` loops and explicit type casts (`int`, `float64`, `string`, `bool`)
//! - local imports & module namespacing: `import "utils/math_helpers.gs"` then
//!   `math_helpers.Lerp(...)`; files are supplied by a [`ScriptResolver`]
//!
//! Example:
//! ```
//! use goscript::value::Value;
//! use goscript::vm::VirtualMachine;
//!
//! let source = r#"
//! var hp = 100
//! func TakeDamage(n int) int {
//!     hp = hp - n
//!     return hp
//! }
//! "#;
//!
//! let mut vm = VirtualMachine::new();
//! let chunk = vm.compile(source)?;
//! vm.execute(chunk)?;
//! let hp = vm.call("TakeDamage", vec![Value::Int(35)])?;
//! assert_eq!(hp.to_string(), "65");
//! # Ok::<(), goscript::error::Error>(())
//! ```

pub mod ast;
pub mod compiler;
pub mod error;
pub mod function;
pub mod hot_reload;
pub mod lexer;
pub mod opcode;
pub mod parser;
pub mod resolver;
pub mod value;
pub mod vm;

pub use compiler::Compiler;
pub use error::Error;
pub use function::CompiledFunction;
pub use hot_reload::HotReloadEngine;
pub use opcode::OpCode;
pub use resolver::{extract_module_name, DiskScriptResolver, MemoryScriptResolver, ScriptResolver};
pub use value::Value;
pub use vm::VirtualMachine;