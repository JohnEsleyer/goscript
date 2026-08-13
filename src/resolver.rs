//! Local import & module system.
//!
//! GoScript scripts can pull in sibling files with `import "path/file.gos"`
//! or entire package directories with `import "pkg/math"`.  The
//! [`ScriptResolver`] trait lets host engines supply file contents (the real
//! filesystem, embedded bytes, or a virtual archive); [`DiskScriptResolver`]
//! reads from disk by default.
//!
//! Imports are namespaced: importing `utils/math_helpers.gos` makes its
//! top-level functions and variables available as `math_helpers.Lerp(...)` and
//! `math_helpers.SomeGlobal`. Circular import chains are rejected.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::ast::{Expr, Stmt};
use crate::error::Error;
use crate::lexer::Lexer;

/// Trait implemented by host engines (Groot) to supply script files by path.
/// `resolve` returns a single file's contents.
/// `resolve_package` returns all files in a package directory (or a single file).
pub trait ScriptResolver {
    fn resolve(&self, path: &str) -> Result<String, String> {
        let files = self.resolve_package(path)?;
        files
            .into_iter()
            .next()
            .map(|(_, content)| content)
            .ok_or_else(|| format!("file not found: '{path}'"))
    }

    /// Resolves a path (file or package directory).
    /// Returns a list of (file_path, file_contents) pairs for all package files.
    fn resolve_package(&self, path: &str) -> Result<Vec<(String, String)>, String>;
}

/// Default filesystem-backed resolver.
pub struct DiskScriptResolver;

impl ScriptResolver for DiskScriptResolver {
    fn resolve_package(&self, path: &str) -> Result<Vec<(String, String)>, String> {
        let p = Path::new(path);
        if p.is_dir() {
            let entries =
                fs::read_dir(p).map_err(|e| format!("cannot read directory '{path}': {e}"))?;
            let mut paths: Vec<_> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.is_file()
                        && p.extension()
                            .map_or(false, |ext| ext == "go" || ext == "gos")
                })
                .collect();
            paths.sort();
            let mut files = Vec::new();
            for file_path in paths {
                let content = fs::read_to_string(&file_path)
                    .map_err(|e| format!("cannot read '{}': {e}", file_path.display()))?;
                files.push((file_path.to_string_lossy().to_string(), content));
            }
            if files.is_empty() {
                return Err(format!("no .go or .gos files found in directory '{path}'"));
            }
            Ok(files)
        } else if p.is_file() {
            let content =
                fs::read_to_string(p).map_err(|e| format!("cannot read '{path}': {e}"))?;
            Ok(vec![(path.to_string(), content)])
        } else {
            // Try appending .go / .gos extensions
            let go_path = format!("{path}.go");
            if Path::new(&go_path).is_file() {
                let content = fs::read_to_string(&go_path)
                    .map_err(|e| format!("cannot read '{go_path}': {e}"))?;
                return Ok(vec![(go_path, content)]);
            }
            let gos_path = format!("{path}.gos");
            if Path::new(&gos_path).is_file() {
                let content = fs::read_to_string(&gos_path)
                    .map_err(|e| format!("cannot read '{gos_path}': {e}"))?;
                return Ok(vec![(gos_path, content)]);
            }
            Err(format!("cannot resolve package or file path '{path}'"))
        }
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
    fn resolve_package(&self, path: &str) -> Result<Vec<(String, String)>, String> {
        // Check if it's a directory-like prefix (e.g. "pkg/math" matches "pkg/math/vector.go")
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{path}/")
        };
        let mut dir_files: Vec<_> = self
            .files
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if !dir_files.is_empty() {
            dir_files.sort_by(|a, b| a.0.cmp(&b.0));
            return Ok(dir_files);
        }
        // Try as a single file
        if let Some(content) = self.files.get(path) {
            return Ok(vec![(path.to_string(), content.clone())]);
        }
        // Try with .go / .gos extension
        let go_path = format!("{path}.go");
        if let Some(content) = self.files.get(&go_path) {
            return Ok(vec![(go_path, content.clone())]);
        }
        let gos_path = format!("{path}.gos");
        if let Some(content) = self.files.get(&gos_path) {
            return Ok(vec![(gos_path, content.clone())]);
        }
        Err(format!("file not found: '{path}'"))
    }
}

/// Derives the namespace name from a script path.
/// `"utils/math_helpers.gos"` -> `"math_helpers"`.
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
            Stmt::Package { .. } => {
                // Metadata for namespace resolution; stripped from runtime execution.
            }
            Stmt::Import { path, alias, line } => {
                if visited.contains(&path) {
                    return Err(Error::new(
                        format!("circular import detected ('{path}' already in the chain)"),
                        line,
                        0,
                    ));
                }

                // Resolve all files in the package (directory or single file)
                let pkg_files = resolver.resolve_package(&path).map_err(|e| {
                    Error::new(format!("cannot resolve import '{path}': {e}"), line, 0)
                })?;

                let mut combined_ast = Vec::new();
                let mut declared_pkg = None;

                for (file_path, source) in &pkg_files {
                    let mut lexer = Lexer::new(source);
                    let tokens = lexer.tokenize()?;
                    let ast = crate::parser::parse(tokens)?;

                    if declared_pkg.is_none() {
                        declared_pkg = ast.iter().find_map(|s| match s {
                            Stmt::Package { name, .. } => Some(name.clone()),
                            _ => None,
                        });
                    }

                    visited.insert(file_path.clone());
                    combined_ast.extend(ast);
                }

                // Resolution priority: 1. Import alias -> 2. Declared package -> 3. File stem
                let name = alias.unwrap_or_else(|| {
                    declared_pkg.unwrap_or_else(|| extract_module_name(&path))
                });

                if modules.contains(&name) {
                    return Err(Error::new(
                        format!("duplicate import of module '{name}'"),
                        line,
                        0,
                    ));
                }

                let imported = resolve_file(combined_ast, Some(&name), resolver, visited)?;
                out.extend(imported);
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
    resolve_file(stmts, module_name, resolver, visited)
}

/// Prefixes a top-level declaration's name with `prefix` (imported modules).
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
// Dotted-reference rewriting
// ---------------------------------------------------------------------------

fn rewrite_import_refs_block(block: Vec<Stmt>, modules: &HashSet<String>) -> Vec<Stmt> {
    block
        .into_iter()
        .map(|s| rewrite_import_refs(s, modules))
        .collect()
}

fn rewrite_import_refs(stmt: Stmt, modules: &HashSet<String>) -> Stmt {
    match stmt {
        Stmt::Package { .. } => stmt,
        Stmt::Import { .. } => stmt,
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
        Stmt::Package { .. } => stmt,
        Stmt::Import { .. } => stmt,
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
