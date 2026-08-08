use crate::value::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOp {
    Negate,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogicalOp {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Value),
    Identifier(String),
    StructInit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    GetField {
        object: Box<Expr>,
        field: String,
    },
    GetIndex {
        object: Box<Expr>,
        index: Box<Expr>,
    },
    SliceInit {
        items: Vec<Expr>,
    },
    MapInit {
        entries: Vec<(Expr, Expr)>,
    },
    Call {
        callee: String,
        args: Vec<Expr>,
    },
    MethodCall {
        receiver: Box<Expr>,
        method: String,
        args: Vec<Expr>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Logical {
        left: Box<Expr>,
        op: LogicalOp,
        right: Box<Expr>,
    },
}

pub type Block = Vec<Stmt>;

#[derive(Debug, Clone)]
pub enum Stmt {
    VarDecl {
        name: String,
        init: Option<Expr>,
        line: usize,
    },
    Assign {
        name: String,
        value: Expr,
        line: usize,
    },
    SetField {
        object: Expr,
        field: String,
        value: Expr,
        line: usize,
    },
    SetIndex {
        object: Expr,
        index: Expr,
        value: Expr,
        line: usize,
    },
    If {
        condition: Expr,
        then_branch: Block,
        else_branch: Option<Block>,
        line: usize,
    },
    For {
        init: Option<Box<Stmt>>,
        condition: Option<Expr>,
        post: Option<Box<Stmt>>,
        body: Block,
        line: usize,
    },
    Switch {
        expr: Expr,
        cases: Vec<(Expr, Block)>,
        default_case: Option<Block>,
        line: usize,
    },
    FuncDecl {
        name: String,
        receiver: Option<(String, String)>,
        params: Vec<String>,
        body: Block,
        line: usize,
    },
    StructDecl {
        name: String,
        fields: Vec<String>,
        line: usize,
    },
    Return(Option<Expr>, usize),
    Break(usize),
    Continue(usize),
    Expr(Expr, usize),
}