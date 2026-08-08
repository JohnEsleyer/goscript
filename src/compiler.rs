use std::collections::HashSet;
use std::rc::Rc;

use crate::ast::{BinaryOp, Expr, LogicalOp, Stmt, UnaryOp};
use crate::function::CompiledFunction;
use crate::opcode::OpCode;
use crate::value::Value;

struct LoopContext {
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
}

pub struct Compiler {
    function: CompiledFunction,
    locals: Vec<String>,
    scope_depth: usize,
    loops: Vec<LoopContext>,
    /// Statement line for the next emitted opcode (P1 line tracking).
    current_line: usize,
    pub preserve_existing: HashSet<String>,
}

impl Compiler {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            function: CompiledFunction::new(name),
            locals: Vec::new(),
            scope_depth: 0,
            loops: Vec::new(),
            current_line: 0,
            preserve_existing: HashSet::new(),
        }
    }

    pub fn with_preserved_globals(name: impl Into<String>, preserve: HashSet<String>) -> Self {
        Self {
            preserve_existing: preserve,
            ..Self::new(name)
        }
    }

    pub fn compile(mut self, stmts: &[Stmt]) -> CompiledFunction {
        self.compile_block(stmts);
        self.emit_op(OpCode::Return);
        self.function
    }

    fn emit_op(&mut self, op: OpCode) {
        self.function.code.push(op as u8);
        self.function.lines.push(self.current_line);
    }

    fn emit_byte(&mut self, byte: u8) {
        self.function.code.push(byte);
        self.function.lines.push(self.current_line);
    }

    fn emit_constant(&mut self, value: Value) {
        self.function.constants.push(value);
        let idx = (self.function.constants.len() - 1) as u8;
        self.emit_op(OpCode::Constant);
        self.emit_byte(idx);
    }

    fn name_constant(&mut self, name: &str) -> u8 {
        self.function.constants.push(Value::String(name.to_string()));
        (self.function.constants.len() - 1) as u8
    }

    fn emit_jump_placeholder(&mut self, op: OpCode) -> usize {
        self.emit_op(op);
        let operand_pos = self.function.code.len();
        self.emit_byte(0);
        operand_pos
    }

    fn patch_jump(&mut self, operand_pos: usize, target: usize) {
        let delta = target as i64 - (operand_pos as i64 + 1);
        self.function.code[operand_pos] = delta as i8 as u8;
    }

    fn emit_back_jump(&mut self, op: OpCode, target: usize) {
        self.emit_op(op);
        let operand_pos = self.function.code.len();
        let delta = target as i64 - (operand_pos as i64 + 1);
        self.emit_byte(delta as i8 as u8);
    }

    fn resolve_local(&self, name: &str) -> Option<u8> {
        self.locals.iter().rposition(|l| l == name).map(|i| i as u8)
    }

    fn compile_block(&mut self, block: &[Stmt]) {
        for stmt in block {
            self.compile_stmt(stmt);
        }
    }

    fn compile_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::VarDecl { name, init, line } => {
                self.current_line = *line;
                if self.scope_depth > 0 {
                    // Local variable.
                    self.locals.push(name.clone());
                    match init {
                        Some(expr) => self.compile_expr(expr),
                        None => self.emit_op(OpCode::Nil),
                    }
                    self.emit_op(OpCode::SetLocal);
                    self.emit_byte((self.locals.len() - 1) as u8);
                } else if self.preserve_existing.contains(name) {
                    // Hot reload: keep the live runtime value of an existing global.
                } else {
                    match init {
                        Some(expr) => self.compile_expr(expr),
                        None => self.emit_op(OpCode::Nil),
                    }
                    let idx = self.name_constant(name);
                    self.emit_op(OpCode::SetGlobal);
                    self.emit_byte(idx);
                }
            }
            Stmt::Assign { name, value, line } => {
                self.current_line = *line;
                self.compile_expr(value);
                if let Some(local_idx) = self.resolve_local(name) {
                    self.emit_op(OpCode::SetLocal);
                    self.emit_byte(local_idx);
                } else {
                    let idx = self.name_constant(name);
                    self.emit_op(OpCode::SetGlobal);
                    self.emit_byte(idx);
                }
            }
            Stmt::SetField { object, field, value, line } => {
                self.current_line = *line;
                self.compile_expr(object);
                self.compile_expr(value);
                let idx = self.name_constant(field);
                self.emit_op(OpCode::SetField);
                self.emit_byte(idx);
            }
            Stmt::SetIndex { object, index, value, line } => {
                self.current_line = *line;
                self.compile_expr(object);
                self.compile_expr(index);
                self.compile_expr(value);
                self.emit_op(OpCode::SetIndex);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                line,
            } => {
                self.current_line = *line;
                self.compile_expr(condition);
                let cond_jump = self.emit_jump_placeholder(OpCode::JumpIfFalse);
                self.compile_block(then_branch);
                if let Some(else_block) = else_branch {
                    let end_jump = self.emit_jump_placeholder(OpCode::Jump);
                    let after_then = self.function.code.len();
                    self.patch_jump(cond_jump, after_then);
                    self.compile_block(else_block);
                    let end = self.function.code.len();
                    self.patch_jump(end_jump, end);
                } else {
                    let end = self.function.code.len();
                    self.patch_jump(cond_jump, end);
                }
            }
            Stmt::For {
                init,
                condition,
                post,
                body,
                line,
            } => {
                self.current_line = *line;
                if let Some(init) = init {
                    self.compile_stmt(init);
                }
                let loop_start = self.function.code.len();
                let cond_jump = if let Some(cond) = condition {
                    self.compile_expr(cond);
                    Some(self.emit_jump_placeholder(OpCode::JumpIfFalse))
                } else {
                    None
                };

                self.loops.push(LoopContext {
                    break_jumps: Vec::new(),
                    continue_jumps: Vec::new(),
                });
                self.compile_block(body);

                let continue_target = if post.is_some() {
                    self.function.code.len()
                } else {
                    loop_start
                };
                if let Some(post) = post {
                    self.compile_stmt(post);
                }
                self.emit_back_jump(OpCode::Jump, loop_start);

                let end = self.function.code.len();
                if let Some(cond_operand) = cond_jump {
                    self.patch_jump(cond_operand, end);
                }
                let ctx = self.loops.pop().unwrap();
                for operand_pos in &ctx.break_jumps {
                    self.patch_jump(*operand_pos, end);
                }
                for operand_pos in &ctx.continue_jumps {
                    self.patch_jump(*operand_pos, continue_target);
                }
            }
            Stmt::Switch {
                expr,
                cases,
                default_case,
                line,
            } => {
                self.current_line = *line;
                // Evaluate the switch expression per case and jump to the
                // matching one. Each failing case falls through to the next.
                let mut case_exits = Vec::new();
                for (case_expr, body) in cases {
                    self.compile_expr(expr);
                    self.compile_expr(case_expr);
                    self.emit_op(OpCode::Equal);
                    let next_case = self.emit_jump_placeholder(OpCode::JumpIfFalse);
                    self.compile_block(body);
                    let exit = self.emit_jump_placeholder(OpCode::Jump);
                    case_exits.push(exit);
                    let after_case = self.function.code.len();
                    self.patch_jump(next_case, after_case);
                }
                if let Some(default_body) = default_case {
                    self.compile_block(default_body);
                }
                let end = self.function.code.len();
                for operand in case_exits {
                    self.patch_jump(operand, end);
                }
            }
            Stmt::FuncDecl {
                name,
                receiver,
                params,
                body,
                line,
            } => {
                self.current_line = *line;
                // Receiver methods live under `Type.Method` so runtime
                // method dispatch can find them from the struct's type name.
                let fn_name = match receiver {
                    Some((_, type_name)) => format!("{type_name}.{name}"),
                    None => name.clone(),
                };
                let mut inner = Compiler::new(fn_name.clone());
                inner.scope_depth = 1;
                if let Some((recv_name, _)) = receiver {
                    // The receiver is the first parameter (local 0).
                    inner.locals.push(recv_name.clone());
                }
                for param in params {
                    inner.locals.push(param.clone());
                }
                inner.function.arity = inner.locals.len();
                let compiled_fn = inner.compile(body);
                self.emit_constant(Value::Function(Rc::new(compiled_fn)));
                let idx = self.name_constant(&fn_name);
                self.emit_op(OpCode::SetGlobal);
                self.emit_byte(idx);
            }
            Stmt::StructDecl { line, .. } => {
                // Struct types are compile-time metadata only; struct instances
                // keep their own dynamic field maps at runtime.
                self.current_line = *line;
            }
            Stmt::Return(value, line) => {
                self.current_line = *line;
                match value {
                Some(expr) => {
                    self.compile_expr(expr);
                    self.emit_op(OpCode::Return);
                }
                None => {
                    self.emit_op(OpCode::Nil);
                    self.emit_op(OpCode::Return);
                }
            }
            }
            Stmt::Break(line) => {
                self.current_line = *line;
                let operand_pos = self.emit_jump_placeholder(OpCode::Jump);
                if let Some(ctx) = self.loops.last_mut() {
                    ctx.break_jumps.push(operand_pos);
                }
            }
            Stmt::Continue(line) => {
                self.current_line = *line;
                let operand_pos = self.emit_jump_placeholder(OpCode::Jump);
                if let Some(ctx) = self.loops.last_mut() {
                    ctx.continue_jumps.push(operand_pos);
                }
            }
            Stmt::Expr(expr, line) => {
                self.current_line = *line;
                self.compile_expr(expr);
                self.emit_op(OpCode::Pop);
            }
        }
    }

    fn compile_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Literal(Value::Nil) => self.emit_op(OpCode::Nil),
            Expr::Literal(Value::Bool(true)) => self.emit_op(OpCode::True),
            Expr::Literal(Value::Bool(false)) => self.emit_op(OpCode::False),
            Expr::Literal(other) => self.emit_constant(other.clone()),
            Expr::Identifier(name) => {
                if let Some(local_idx) = self.resolve_local(name) {
                    self.emit_op(OpCode::GetLocal);
                    self.emit_byte(local_idx);
                } else {
                    let idx = self.name_constant(name);
                    self.emit_op(OpCode::GetGlobal);
                    self.emit_byte(idx);
                }
            }
            Expr::StructInit { name, fields } => {
                for (field_name, field_value) in fields {
                    let _ = self.emit_constant(Value::String(field_name.clone()));
                    self.compile_expr(field_value);
                }
                let name_idx = self.name_constant(name);
                self.emit_op(OpCode::NewStruct);
                self.emit_byte(name_idx);
                self.emit_byte(fields.len() as u8);
            }
            Expr::GetField { object, field } => {
                self.compile_expr(object);
                let idx = self.name_constant(field);
                self.emit_op(OpCode::GetField);
                self.emit_byte(idx);
            }
            Expr::GetIndex { object, index } => {
                self.compile_expr(object);
                self.compile_expr(index);
                self.emit_op(OpCode::GetIndex);
            }
            Expr::SliceInit { items } => {
                for item in items {
                    self.compile_expr(item);
                }
                self.emit_op(OpCode::NewSlice);
                self.emit_byte(items.len() as u8);
            }
            Expr::MapInit { entries } => {
                for (key, value) in entries {
                    self.compile_expr(key);
                    self.compile_expr(value);
                }
                self.emit_op(OpCode::NewMap);
                self.emit_byte(entries.len() as u8);
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                self.compile_expr(receiver);
                for arg in args {
                    self.compile_expr(arg);
                }
                let idx = self.name_constant(method);
                self.emit_op(OpCode::CallMethod);
                self.emit_byte(idx);
                self.emit_byte(args.len() as u8);
            }
            Expr::Call { callee, args } => {
                for arg in args {
                    self.compile_expr(arg);
                }
                let idx = self.name_constant(callee);
                self.emit_op(OpCode::Call);
                self.emit_byte(idx);
                self.emit_byte(args.len() as u8);
            }
            Expr::Unary { op, expr } => {
                self.compile_expr(expr);
                match op {
                    UnaryOp::Negate => self.emit_op(OpCode::Negate),
                    UnaryOp::Not => self.emit_op(OpCode::Not),
                }
            }
            Expr::Binary { left, op, right } => {
                self.compile_expr(left);
                self.compile_expr(right);
                let opcode = match op {
                    BinaryOp::Add => OpCode::Add,
                    BinaryOp::Sub => OpCode::Sub,
                    BinaryOp::Mul => OpCode::Mul,
                    BinaryOp::Div => OpCode::Div,
                    BinaryOp::Rem => OpCode::Mod,
                    BinaryOp::Equal => OpCode::Equal,
                    BinaryOp::NotEqual => OpCode::NotEqual,
                    BinaryOp::Less => OpCode::Less,
                    BinaryOp::LessEqual => OpCode::LessEqual,
                    BinaryOp::Greater => OpCode::Greater,
                    BinaryOp::GreaterEqual => OpCode::GreaterEqual,
                };
                self.emit_op(opcode);
            }
            Expr::Logical { left, op, right } => {
                self.compile_expr(left);
                match op {
                    LogicalOp::And => {
                        let false_jump = self.emit_jump_placeholder(OpCode::JumpIfFalseKeep);
                        self.emit_op(OpCode::Pop);
                        self.compile_expr(right);
                        let end = self.function.code.len();
                        self.patch_jump(false_jump, end);
                    }
                    LogicalOp::Or => {
                        let true_jump = self.emit_jump_placeholder(OpCode::JumpIfTrueKeep);
                        self.emit_op(OpCode::Pop);
                        self.compile_expr(right);
                        let end = self.function.code.len();
                        self.patch_jump(true_jump, end);
                    }
                }
            }
        }
    }
}