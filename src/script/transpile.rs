//! TypeScript → JavaScript (types stripped by oxc), with a line map so errors point at
//! the original source.

use std::path::Path;

use anyhow::{bail, Result};
use oxc::allocator::Allocator;
use oxc::codegen::{Codegen, CodegenOptions};
use oxc::parser::Parser;
use oxc::semantic::SemanticBuilder;
use oxc::span::SourceType;
use oxc::transformer::{TransformOptions, Transformer};

/// Generated JS plus, for each generated line (0-based), the original line (0-based).
pub struct Transpiled {
    pub code: String,
    pub lines: Vec<u32>,
}

impl Transpiled {
    /// Map a 1-based generated line to a 1-based source line.
    pub fn source_line(&self, line: u32) -> u32 {
        if self.lines.is_empty() {
            return line;
        }
        let i = (line.saturating_sub(1) as usize).min(self.lines.len() - 1);
        self.lines[i] + 1
    }
}

fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let before = &src[..offset.min(src.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1;
    (line, col)
}

fn describe(src: &str, path: &Path, diags: &oxc::diagnostics::Diagnostics) -> String {
    let mut out = Vec::new();
    for e in diags.errors().take(5) {
        let at = e
            .labels
            .first()
            .map(|l| {
                let (line, col) = line_col(src, l.offset() as usize);
                format!("{}:{line}:{col}: ", path.display())
            })
            .unwrap_or_else(|| format!("{}: ", path.display()));
        out.push(format!("{at}{}", e.message));
    }
    out.join("\n")
}

/// Strip TypeScript syntax. Plain `.js`/`.mjs` sources pass through unchanged.
pub fn transpile(source: &str, path: &Path) -> Result<Transpiled> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("ts").to_ascii_lowercase();
    if matches!(ext.as_str(), "js" | "mjs") {
        return Ok(Transpiled { code: source.to_string(), lines: Vec::new() });
    }
    let allocator = Allocator::default();
    let source_type = SourceType::ts().with_module(true);
    let ret = Parser::new(&allocator, source, source_type).parse();
    if ret.diagnostics.has_errors() {
        bail!("syntax error\n{}", describe(source, path, &ret.diagnostics));
    }
    let mut program = ret.program;
    let sem = SemanticBuilder::new().with_excess_capacity(2.0).build(&program);
    if sem.diagnostics.has_errors() {
        bail!("{}", describe(source, path, &sem.diagnostics));
    }
    let scoping = sem.semantic.into_scoping();
    let options = TransformOptions::default();
    let tr = Transformer::new(&allocator, path, &options).build_with_scoping(scoping, &mut program);
    if tr.diagnostics.has_errors() {
        bail!("{}", describe(source, path, &tr.diagnostics));
    }
    let cg = Codegen::new()
        .with_options(CodegenOptions { source_map_path: Some(path.to_path_buf()), ..CodegenOptions::default() })
        .build(&program);
    let code = cg.code;
    let n_lines = code.matches('\n').count() + 1;
    let mut lines: Vec<Option<u32>> = vec![None; n_lines];
    if let Some(map) = cg.map {
        for t in map.get_tokens() {
            let d = t.get_dst_line() as usize;
            if d < lines.len() && lines[d].is_none() {
                lines[d] = Some(t.get_src_line());
            }
        }
    }
    // Lines without a token take the previous mapped line.
    let mut last = 0;
    let lines = lines
        .into_iter()
        .map(|l| {
            if let Some(v) = l {
                last = v;
            }
            last
        })
        .collect();
    Ok(Transpiled { code, lines })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_types_and_maps_lines() {
        let src = "interface P { x: number }\n\nconst a: number = 1;\nfunction f(p: P): number {\n  return p.x + a;\n}\nexport { f };\n";
        let t = transpile(src, Path::new("x.ts")).unwrap();
        assert!(!t.code.contains("interface"));
        assert!(t.code.contains("function f(p)"));
        let gen_line = t.code.lines().position(|l| l.contains("return")).unwrap() as u32 + 1;
        assert_eq!(t.source_line(gen_line), 5);
        let err = transpile("const x: = 3;", Path::new("bad.ts")).err().unwrap().to_string();
        assert!(err.contains("bad.ts:1:"), "{err}");
    }
}
