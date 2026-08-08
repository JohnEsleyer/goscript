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
pub mod value;
pub mod vm;

pub use compiler::Compiler;
pub use error::Error;
pub use function::CompiledFunction;
pub use hot_reload::HotReloadEngine;
pub use opcode::OpCode;
pub use value::Value;
pub use vm::VirtualMachine;