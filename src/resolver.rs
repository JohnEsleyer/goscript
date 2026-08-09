//! Local import & module system.
//!
//! GoScript scripts can pull in sibling files with `import "path/file.go"`.
//! The [`ScriptResolver`] trait lets host engines supply file contents (the
//! real filesystem, embedded bytes, or a virtual archive);
//! [`DiskScriptResolver`] reads from disk by default.
//!
//! Imports are namespaced: importing `utils/math_helpers.go` makes its
//! top-level functions and variables available as `math_helpers.Lerp(...)` and
//! `math_helpers.SomeGlobal`. Circular import chains are rejected.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::ast::{Expr, Stmt};
use crate::error::Error;
use crate::lexer::Lexer;

/// Trait implemented by host engines (Groot/Bevy) to supply script files by
/// path. `resolve` returns the file contents, or an error string.
pub trait ScriptResolver {
    fn resolve(&self, path: &str) -> Result<String, String>;
}

/// Default filesystem-backed resolver: `fs::read_to_string(path)`.
pub struct DiskScriptResolver;

impl ScriptResolver for DiskScriptResolver {
    fn resolve(&self, path: &str) -> Result<String, String> {
        fs::read_to_string(path).map_err(|e| format!("cannot read '{path}': {e}"))
    }
}

/// In-memory resolver for tests and virtual file systems.
pub struct MemoryScriptResolver {
    files: HashMap<String, String>,
}

impl MemoryScriptResolver {
    pub fn new(files: HashMap<String, String>) -> Self {
        Self { files }
    }
}

impl ScriptResolver for MemoryScriptResolver {
    fn resolve(&self, path: &str) -> Result<String, String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("file not found: '{path}'"))
    }
}

/// Derives the namespace name from a script path.
/// `"utils/math_helpers.go"` -> `"math_helpers"`.
pub fn extract_module_name(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.to_string())
}

/// Resolves every top-level `Stmt::Import` in a parsed program, returning a
/// flattened statement list. Imported module bodies are spliced in at the
/// import sites with their top-level functions/variables namespaced under
/// `module_name.`, and dotted references to imported modules are rewritten
/// into dotted global names.
pub fn resolve_imports(
    stmts: Vec<Stmt>,
    resolver: &dyn ScriptResolver,
) -> Result<Vec<Stmt>, Error> {
    let mut visited = HashSet::new();
    resolve_file(stmts, None, resolver, &mut visited)
}

/// Same as [`resolve_imports`] but also returns the set of all imported file
/// paths (recursively). Used by [`HotReloadEngine`] to watch dependencies.
pub fn resolve_imports_with_deps(
    stmts: Vec<Stmt>,
    resolver: &dyn ScriptResolver,
) -> Result<(Vec<Stmt>, HashSet<String>), Error> {
    let mut visited = HashSet::new();
    let deps = HashSet::new();
    let resolved = resolve_file_with_deps(stmts, None, resolver, &mut visited, &deps)?;
    Ok((resolved, visited))
}

/// Processes one file. `module_name` is `None` for the root program (its
/// symbols stay bare) and `Some(name)` for imported files, whose top-level
/// declarations get prefixed and self-referenced internally.
fn resolve_file(
    stmts: Vec<Stmt>,
    module_name: Option<&str>,
    resolver: &dyn ScriptResolver,
    visited: &mut HashSet<String>,
) -> Result<Vec<Stmt>, Error> {
    let mut modules: HashSet<String> = HashSet::new();
    let mut out: Vec<Stmt> = Vec::new();

    for stmt in stmts {
        match stmt {
            Stmt::Import { path, line } => {
                let name = extract_module_name(&path);
                if modules.contains(&name) {
                    return Err(Error::new(
                        format!("duplicate import of module '{name}'"),
                        line,
                        0,
                    ));
                }
                if visited.contains(&path) {
                    return Err(Error::new(
                        format!("circular import detected ('{path}' already in the chain)"),
                        line,
                        0,
                    ));
                }
                let source = resolver.resolve(&path).map_err(|e| {
                    Error::new(format!("cannot resolve import '{path}': {e}"), line, 0)
                })?;
                let mut lexer = Lexer::new(&source);
                let tokens = lexer.tokenize()?;
                let ast = crate::parser::parse(tokens)?;
                visited.insert(path.clone());
                let imported = resolve_file(ast, Some(&name), resolver, visited)?;
                out.extend(imported);
                // Future parses (e.g. hot reload) see dotted module access as
                // a package call rather than a struct field/method read.
                crate::parser::declare_package(&name);
                modules.insert(name);
            }
            other => out.push(other),
        }
    }

    // Rewrite dotted references to imported submodules in this file's code.
    let out: Vec<Stmt> = out
        .into_iter()
        .map(|s| rewrite_import_refs(s, &modules))
        .collect();

    match module_name {
        None => Ok(out),
        Some(prefix) => {
            // Namespace this file's own top-level declarations and qualify its
            // internal references to them.
            let mut own: HashSet<String> = HashSet::new();
            let scoped: Vec<Stmt> = out
                .into_iter()
                .map(|s| {
                    match &s {
                        Stmt::FuncDecl { name, .. } | Stmt::VarDecl { name, .. } => {
                            own.insert(name.clone());
                        }
                        _ => {}
                    }
                    prefix_decl(s, prefix)
                })
                .collect();
            Ok(scoped
                .into_iter()
                .map(|s| rewrite_self_refs(s, &own, prefix))
                .collect())
        }
    }
}

/// Same as [`resolve_file`] but returns both the resolved statements and the
/// set of all imported file paths (the dependency set).
fn resolve_file_with_deps(
    stmts: Vec<Stmt>,
    module_name: Option<&str>,
    resolver: &dyn ScriptResolver,
    visited: &mut HashSet<String>,
    _deps: &HashSet<String>,
) -> Result<Vec<Stmt>, Error> {
    // resolve_file already populates `visited` with all imported paths.
    resolve_file(stmts, module_name, resolver, visited)
}

/// Prefixes a top-level declaration's name with `prefix` (imported modules).
/// Receiver methods keep their `<Type>.<Method>` registration and struct types
/// are compile-time metadata only, so neither is namespaced.
fn prefix_decl(stmt: Stmt, prefix: &str) -> Stmt {
    match stmt {
        Stmt::FuncDecl { name, receiver: None, params, body, line } => Stmt::FuncDecl {
            name: format!("{prefix}.{name}"),
            receiver: None,
            params,
            body,
            line,
        },
        Stmt::VarDecl { name, init, line } => Stmt::VarDecl {
            name: format!("{prefix}.{name}"),
            init,
            line,
        },
        other => other,
    }
}

// ---------------------------------------------------------------------------
// Dotted-reference rewriting: `submod.X(...)` / `submod.X` become dotted
// global names so calls hit the namespaced module globals.
// ---------------------------------------------------------------------------

fn rewrite_import_refs_block(block: Vec<Stmt>, modules: &HashSet<String>) -> Vec<Stmt> {
    block
        .into_iter()
        .map(|s| rewrite_import_refs(s, modules))
        .collect()
}

fn rewrite_import_refs(stmt: Stmt, modules: &HashSet<String>) -> Stmt {
    match stmt {
        Stmt::VarDecl { name, init, line } => Stmt::VarDecl {
            name,
            init: init.map(|e| rewrite_import_refs_expr(e, modules)),
            line,
        },
        Stmt::Assign { name, value, line } => Stmt::Assign {
            name,
            value: rewrite_import_refs_expr(value, modules),
            line,
        },
        Stmt::SetField { object, field, value, line } => {
            if let Expr::Identifier(m) = &object {
                if modules.contains(m) {
                    return Stmt::Assign {
                        name: format!("{m}.{field}"),
                        value: rewrite_import_refs_expr(value, modules),
                        line,
                    };
                }
            }
            Stmt::SetField {
                object: rewrite_import_refs_expr(object, modules),
                field,
                value: rewrite_import_refs_expr(value, modules),
                line,
            }
        }
        Stmt::SetIndex { object, index, value, line } => Stmt::SetIndex {
            object: rewrite_import_refs_expr(object, modules),
            index: rewrite_import_refs_expr(index, modules),
            value: rewrite_import_refs_expr(value, modules),
            line,
        },
        Stmt::If { condition, then_branch, else_branch, line } => Stmt::If {
            condition: rewrite_import_refs_expr(condition, modules),
            then_branch: rewrite_import_refs_block(then_branch, modules),
            else_branch: else_branch.map(|b| rewrite_import_refs_block(b, modules)),
            line,
        },
        Stmt::For { init, condition, post, body, line } => Stmt::For {
            init: init.map(|i| Box::new(rewrite_import_refs(*i, modules))),
            condition: condition.map(|c| rewrite_import_refs_expr(c, modules)),
            post: post.map(|p| Box::new(rewrite_import_refs(*p, modules))),
            body: rewrite_import_refs_block(body, modules),
            line,
        },
        Stmt::Switch { expr, cases, default_case, line } => Stmt::Switch {
            expr: rewrite_import_refs_expr(expr, modules),
            cases: cases
                .into_iter()
                .map(|(e, b)| {
                    (
                        rewrite_import_refs_expr(e, modules),
                        rewrite_import_refs_block(b, modules),
                    )
                })
                .collect(),
            default_case: default_case.map(|b| rewrite_import_refs_block(b, modules)),
            line,
        },
        Stmt::FuncDecl { name, receiver, params, body, line } => Stmt::FuncDecl {
            name,
            receiver,
            params,
            body: rewrite_import_refs_block(body, modules),
            line,
        },
        Stmt::StructDecl { name, fields, line } => Stmt::StructDecl { name, fields, line },
        Stmt::Return(e, line) => {
            Stmt::Return(e.map(|e| rewrite_import_refs_expr(e, modules)), line)
        }
        Stmt::Break(line) => Stmt::Break(line),
        Stmt::Continue(line) => Stmt::Continue(line),
        Stmt::Expr(e, line) => Stmt::Expr(rewrite_import_refs_expr(e, modules), line),
        // Imports are resolved above; left as-is defensively.
        Stmt::Import { path, line } => Stmt::Import { path, line },
    }
}

fn rewrite_import_refs_expr(expr: Expr, modules: &HashSet<String>) -> Expr {
    match expr {
        Expr::Literal(_) | Expr::Identifier(_) => expr,
        Expr::StructInit { name, fields } => Expr::StructInit {
            name,
            fields: fields
                .into_iter()
                .map(|(f, e)| (f, rewrite_import_refs_expr(e, modules)))
                .collect(),
        },
        Expr::GetField { object, field } => {
            if let Expr::Identifier(m) = &*object {
                if modules.contains(m) {
                    return Expr::Identifier(format!("{m}.{field}"));
                }
            }
            Expr::GetField {
                object: Box::new(rewrite_import_refs_expr(*object, modules)),
                field,
            }
        }
        Expr::GetIndex { object, index } => Expr::GetIndex {
            object: Box::new(rewrite_import_refs_expr(*object, modules)),
            index: Box::new(rewrite_import_refs_expr(*index, modules)),
        },
        Expr::SliceInit { items } => Expr::SliceInit {
            items: items
                .into_iter()
                .map(|e| rewrite_import_refs_expr(e, modules))
                .collect(),
        },
        Expr::MapInit { entries } => Expr::MapInit {
            entries: entries
                .into_iter()
                .map(|(k, v)| {
                    (
                        rewrite_import_refs_expr(k, modules),
                        rewrite_import_refs_expr(v, modules),
                    )
                })
                .collect(),
        },
        Expr::Call { callee, args } => Expr::Call {
            callee,
            args: args
                .into_iter()
                .map(|a| rewrite_import_refs_expr(a, modules))
                .collect(),
        },
        Expr::MethodCall { receiver, method, args } => {
            if let Expr::Identifier(m) = &*receiver {
                if modules.contains(m) {
                    return Expr::Call {
                        callee: format!("{m}.{method}"),
                        args: args
                            .into_iter()
                            .map(|a| rewrite_import_refs_expr(a, modules))
                            .collect(),
                    };
                }
            }
            Expr::MethodCall {
                receiver: Box::new(rewrite_import_refs_expr(*receiver, modules)),
                method,
                args: args
                    .into_iter()
                    .map(|a| rewrite_import_refs_expr(a, modules))
                    .collect(),
            }
        }
        Expr::Unary { op, expr } => Expr::Unary {
            op,
            expr: Box::new(rewrite_import_refs_expr(*expr, modules)),
        },
        Expr::Binary { left, op, right } => Expr::Binary {
            left: Box::new(rewrite_import_refs_expr(*left, modules)),
            op,
            right: Box::new(rewrite_import_refs_expr(*right, modules)),
        },
        Expr::Logical { left, op, right } => Expr::Logical {
            left: Box::new(rewrite_import_refs_expr(*left, modules)),
            op,
            right: Box::new(rewrite_import_refs_expr(*right, modules)),
        },
    }
}

// ---------------------------------------------------------------------------
// Self-qualification: inside an imported module, bare references to its own
// top-level functions/variables are rewritten to the namespaced dotted name.
// ---------------------------------------------------------------------------

fn rewrite_self_refs(stmt: Stmt, own: &HashSet<String>, prefix: &str) -> Stmt {
    match stmt {
        Stmt::VarDecl { name, init, line } => Stmt::VarDecl {
            name,
            init: init.map(|e| rewrite_self_refs_expr(e, own, prefix)),
            line,
        },
        Stmt::Assign { name, value, line } => {
            let name = if own.contains(&name) {
                format!("{prefix}.{name}")
            } else {
                name
            };
            Stmt::Assign {
                name,
                value: rewrite_self_refs_expr(value, own, prefix),
                line,
            }
        }
        Stmt::SetField { object, field, value, line } => Stmt::SetField {
            object: rewrite_self_refs_expr(object, own, prefix),
            field,
            value: rewrite_self_refs_expr(value, own, prefix),
            line,
        },
        Stmt::SetIndex { object, index, value, line } => Stmt::SetIndex {
            object: rewrite_self_refs_expr(object, own, prefix),
            index: rewrite_self_refs_expr(index, own, prefix),
            value: rewrite_self_refs_expr(value, own, prefix),
            line,
        },
        Stmt::If { condition, then_branch, else_branch, line } => Stmt::If {
            condition: rewrite_self_refs_expr(condition, own, prefix),
            then_branch: rewrite_self_refs_block(then_branch, own, prefix),
            else_branch: else_branch.map(|b| rewrite_self_refs_block(b, own, prefix)),
            line,
        },
        Stmt::For { init, condition, post, body, line } => Stmt::For {
            init: init.map(|i| Box::new(rewrite_self_refs(*i, own, prefix))),
            condition: condition.map(|c| rewrite_self_refs_expr(c, own, prefix)),
            post: post.map(|p| Box::new(rewrite_self_refs(*p, own, prefix))),
            body: rewrite_self_refs_block(body, own, prefix),
            line,
        },
        Stmt::Switch { expr, cases, default_case, line } => Stmt::Switch {
            expr: rewrite_self_refs_expr(expr, own, prefix),
            cases: cases
                .into_iter()
                .map(|(e, b)| {
                    (
                        rewrite_self_refs_expr(e, own, prefix),
                        rewrite_self_refs_block(b, own, prefix),
                    )
                })
                .collect(),
            default_case: default_case.map(|b| rewrite_self_refs_block(b, own, prefix)),
            line,
        },
        Stmt::FuncDecl { name, receiver, params, body, line } => Stmt::FuncDecl {
            name,
            receiver,
            params,
            body: rewrite_self_refs_block(body, own, prefix),
            line,
        },
        Stmt::StructDecl { name, fields, line } => Stmt::StructDecl { name, fields, line },
        Stmt::Return(e, line) => Stmt::Return(e.map(|e| rewrite_self_refs_expr(e, own, prefix)), line),
        Stmt::Break(line) => Stmt::Break(line),
        Stmt::Continue(line) => Stmt::Continue(line),
        Stmt::Expr(e, line) => Stmt::Expr(rewrite_self_refs_expr(e, own, prefix), line),
        Stmt::Import { path, line } => Stmt::Import { path, line },
    }
}

fn rewrite_self_refs_block(block: Vec<Stmt>, own: &HashSet<String>, prefix: &str) -> Vec<Stmt> {
    block
        .into_iter()
        .map(|s| rewrite_self_refs(s, own, prefix))
        .collect()
}

fn rewrite_self_refs_expr(expr: Expr, own: &HashSet<String>, prefix: &str) -> Expr {
    match expr {
        Expr::Identifier(name) => {
            if own.contains(&name) {
                Expr::Identifier(format!("{prefix}.{name}"))
            } else {
                Expr::Identifier(name)
            }
        }
        Expr::Call { callee, args } => {
            let callee = if own.contains(&callee) {
                format!("{prefix}.{callee}")
            } else {
                callee
            };
            Expr::Call {
                callee,
                args: args
                    .into_iter()
                    .map(|a| rewrite_self_refs_expr(a, own, prefix))
                    .collect(),
            }
        }
        Expr::Literal(_) => expr,
        Expr::StructInit { name, fields } => Expr::StructInit {
            name,
            fields: fields
                .into_iter()
                .map(|(f, e)| (f, rewrite_self_refs_expr(e, own, prefix)))
                .collect(),
        },
        Expr::GetField { object, field } => Expr::GetField {
            object: Box::new(rewrite_self_refs_expr(*object, own, prefix)),
            field,
        },
        Expr::GetIndex { object, index } => Expr::GetIndex {
            object: Box::new(rewrite_self_refs_expr(*object, own, prefix)),
            index: Box::new(rewrite_self_refs_expr(*index, own, prefix)),
        },
        Expr::SliceInit { items } => Expr::SliceInit {
            items: items
                .into_iter()
                .map(|e| rewrite_self_refs_expr(e, own, prefix))
                .collect(),
        },
        Expr::MapInit { entries } => Expr::MapInit {
            entries: entries
                .into_iter()
                .map(|(k, v)| {
                    (
                        rewrite_self_refs_expr(k, own, prefix),
                        rewrite_self_refs_expr(v, own, prefix),
                    )
                })
                .collect(),
        },
        Expr::MethodCall { receiver, method, args } => Expr::MethodCall {
            receiver: Box::new(rewrite_self_refs_expr(*receiver, own, prefix)),
            method,
            args: args
                .into_iter()
                .map(|a| rewrite_self_refs_expr(a, own, prefix))
                .collect(),
        },
        Expr::Unary { op, expr } => Expr::Unary {
            op,
            expr: Box::new(rewrite_self_refs_expr(*expr, own, prefix)),
        },
        Expr::Binary { left, op, right } => Expr::Binary {
            left: Box::new(rewrite_self_refs_expr(*left, own, prefix)),
            op,
            right: Box::new(rewrite_self_refs_expr(*right, own, prefix)),
        },
        Expr::Logical { left, op, right } => Expr::Logical {
            left: Box::new(rewrite_self_refs_expr(*left, own, prefix)),
            op,
            right: Box::new(rewrite_self_refs_expr(*right, own, prefix)),
        },
    }
}
