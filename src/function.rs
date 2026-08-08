#[derive(Debug, Clone, PartialEq)]
pub struct CompiledFunction {
    pub name: String,
    pub arity: usize,
    pub code: Vec<u8>,
    pub constants: Vec<crate::value::Value>,
    /// Source line recorded for each bytecode op byte (parallel to `code`).
    /// Used to surface script line numbers on runtime errors.
    pub lines: Vec<usize>,
}

impl CompiledFunction {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            arity: 0,
            code: Vec::new(),
            constants: Vec::new(),
            lines: Vec::new(),
        }
    }
}