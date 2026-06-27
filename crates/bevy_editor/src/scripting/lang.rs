//! The behavior-script language: a lexer, a recursive-descent parser, and a tree-walking
//! evaluator. Everything is `f32`-valued; booleans are represented as `0.0` / `1.0` and any
//! nonzero value is "true". See the parent module docs for the surface syntax.

use std::collections::HashMap;

use bevy_math::{ops, EulerRot, Quat, Vec3};
use bevy_transform::components::Transform;

/// Reserved words that cannot be used as variable names.
const RESERVED: &[&str] = &[
    "let",
    "if",
    "else",
    "true",
    "false",
    "self",
    "spin",
    "rotate",
    "translate",
    "scale",
];

// ---------------------------------------------------------------------------
// AST
// ---------------------------------------------------------------------------

/// A parsed program: a sequence of statements.
#[derive(Debug, Clone)]
pub struct Program {
    stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    Position,
    Rotation,
    Scale,
}

#[derive(Debug, Clone)]
enum LValue {
    Var(String),
    Field(Channel, usize),
    ScaleUniform,
}

#[derive(Debug, Clone)]
enum Stmt {
    Let(String, Expr),
    Assign(LValue, Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    // Legacy one-liners (sugar), applied with `dt`.
    Spin(Expr),
    Rotate(usize, Expr),
    Translate(Expr, Expr, Expr),
    ScaleCmd(Expr),
}

#[derive(Debug, Clone, Copy)]
enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

#[derive(Debug, Clone)]
enum Expr {
    Num(f32),
    Var(String),
    Field(Channel, usize),
    Unary(UnOp, Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f32),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Dot,
    Assign,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Bang,
    /// Statement separator (`;` or newline).
    Sep,
}

fn lex(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' => i += 1,
            '\n' | ';' => {
                out.push(Tok::Sep);
                i += 1;
            }
            '#' => {
                // comment to end of line
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '+' => push(&mut out, &mut i, Tok::Plus),
            '-' => push(&mut out, &mut i, Tok::Minus),
            '*' => push(&mut out, &mut i, Tok::Star),
            '/' => push(&mut out, &mut i, Tok::Slash),
            '%' => push(&mut out, &mut i, Tok::Percent),
            '(' => push(&mut out, &mut i, Tok::LParen),
            ')' => push(&mut out, &mut i, Tok::RParen),
            '{' => push(&mut out, &mut i, Tok::LBrace),
            '}' => push(&mut out, &mut i, Tok::RBrace),
            ',' => push(&mut out, &mut i, Tok::Comma),
            '.' => push(&mut out, &mut i, Tok::Dot),
            '=' => two(&chars, &mut i, &mut out, '=', Tok::EqEq, Tok::Assign),
            '!' => two(&chars, &mut i, &mut out, '=', Tok::Ne, Tok::Bang),
            '<' => two(&chars, &mut i, &mut out, '=', Tok::Le, Tok::Lt),
            '>' => two(&chars, &mut i, &mut out, '=', Tok::Ge, Tok::Gt),
            c if c.is_ascii_digit() || (c == '.') => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let s: String = chars[start..i].iter().collect();
                let n = s
                    .parse::<f32>()
                    .map_err(|_| format!("invalid number `{s}`"))?;
                out.push(Tok::Num(n));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                out.push(Tok::Ident(chars[start..i].iter().collect()));
            }
            other => return Err(format!("unexpected character `{other}`")),
        }
    }
    Ok(out)
}

fn push(out: &mut Vec<Tok>, i: &mut usize, t: Tok) {
    out.push(t);
    *i += 1;
}

fn two(chars: &[char], i: &mut usize, out: &mut Vec<Tok>, next: char, both: Tok, single: Tok) {
    if *i + 1 < chars.len() && chars[*i + 1] == next {
        out.push(both);
        *i += 2;
    } else {
        out.push(single);
        *i += 1;
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

/// Parse a script source string into a [`Program`], or return an error message.
pub fn parse(src: &str) -> Result<Program, String> {
    let toks = lex(src)?;
    let mut p = Parser { toks, pos: 0 };
    let mut stmts = Vec::new();
    p.skip_seps();
    while !p.at_end() {
        stmts.push(p.stmt()?);
        // Statements are separated by `;`/newline; tolerate the last one missing it.
        if !p.at_end() && !p.eat(&Tok::Sep) {
            // allow a closing brace to terminate without a separator
            if p.peek() != Some(&Tok::RBrace) {
                return Err(format!("expected end of statement, found {:?}", p.peek()));
            }
        }
        p.skip_seps();
    }
    Ok(Program { stmts })
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }
    fn advance(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == Some(t) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, t: &Tok) -> Result<(), String> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(format!("expected {t:?}, found {:?}", self.peek()))
        }
    }
    fn skip_seps(&mut self) {
        while self.eat(&Tok::Sep) {}
    }

    fn ident(&mut self) -> Result<String, String> {
        match self.advance() {
            Some(Tok::Ident(s)) => Ok(s),
            other => Err(format!("expected identifier, found {other:?}")),
        }
    }

    fn stmt(&mut self) -> Result<Stmt, String> {
        match self.peek() {
            Some(Tok::Ident(k)) if k == "let" => {
                self.advance();
                let name = self.ident()?;
                if RESERVED.contains(&name.as_str()) {
                    return Err(format!("`{name}` is reserved and cannot be a variable"));
                }
                self.expect(&Tok::Assign)?;
                let e = self.expr()?;
                Ok(Stmt::Let(name, e))
            }
            Some(Tok::Ident(k)) if k == "if" => {
                self.advance();
                let cond = self.expr()?;
                let then = self.block()?;
                let els = if matches!(self.peek(), Some(Tok::Ident(s)) if s == "else") {
                    self.advance();
                    self.block()?
                } else {
                    Vec::new()
                };
                Ok(Stmt::If(cond, then, els))
            }
            Some(Tok::Ident(k)) if k == "spin" => {
                self.advance();
                Ok(Stmt::Spin(self.expr()?))
            }
            Some(Tok::Ident(k)) if k == "rotate" => {
                self.advance();
                let axis = self.axis()?;
                Ok(Stmt::Rotate(axis, self.expr()?))
            }
            Some(Tok::Ident(k)) if k == "translate" => {
                self.advance();
                let x = self.expr()?;
                let y = self.expr()?;
                let z = self.expr()?;
                Ok(Stmt::Translate(x, y, z))
            }
            Some(Tok::Ident(k)) if k == "scale" => {
                self.advance();
                Ok(Stmt::ScaleCmd(self.expr()?))
            }
            _ => {
                let lvalue = self.lvalue()?;
                self.expect(&Tok::Assign)?;
                let e = self.expr()?;
                Ok(Stmt::Assign(lvalue, e))
            }
        }
    }

    fn block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&Tok::LBrace)?;
        let mut stmts = Vec::new();
        self.skip_seps();
        while self.peek() != Some(&Tok::RBrace) {
            if self.at_end() {
                return Err("unclosed `{`".into());
            }
            stmts.push(self.stmt()?);
            if !self.eat(&Tok::Sep) && self.peek() != Some(&Tok::RBrace) {
                return Err(format!(
                    "expected end of statement, found {:?}",
                    self.peek()
                ));
            }
            self.skip_seps();
        }
        self.expect(&Tok::RBrace)?;
        Ok(stmts)
    }

    fn axis(&mut self) -> Result<usize, String> {
        let a = self.ident()?;
        match a.as_str() {
            "x" => Ok(0),
            "y" => Ok(1),
            "z" => Ok(2),
            other => Err(format!("expected axis x|y|z, found `{other}`")),
        }
    }

    /// Parse an assignment target: `self.<channel>[.<comp>]` or a bare variable.
    fn lvalue(&mut self) -> Result<LValue, String> {
        let name = self.ident()?;
        if name == "self" {
            self.expect(&Tok::Dot)?;
            let channel = self.channel()?;
            if self.eat(&Tok::Dot) {
                let comp = self.axis()?;
                Ok(LValue::Field(channel, comp))
            } else if channel == Channel::Scale {
                Ok(LValue::ScaleUniform)
            } else {
                Err("expected `.x`/`.y`/`.z` after self.position/self.rotation".into())
            }
        } else {
            if RESERVED.contains(&name.as_str()) {
                return Err(format!("`{name}` is reserved"));
            }
            Ok(LValue::Var(name))
        }
    }

    fn channel(&mut self) -> Result<Channel, String> {
        let c = self.ident()?;
        match c.as_str() {
            "position" => Ok(Channel::Position),
            "rotation" => Ok(Channel::Rotation),
            "scale" => Ok(Channel::Scale),
            other => Err(format!("unknown channel `self.{other}`")),
        }
    }

    // Expression grammar (lowest to highest precedence).
    fn expr(&mut self) -> Result<Expr, String> {
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let mut left = self.term()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Lt) => BinOp::Lt,
                Some(Tok::Le) => BinOp::Le,
                Some(Tok::Gt) => BinOp::Gt,
                Some(Tok::Ge) => BinOp::Ge,
                Some(Tok::EqEq) => BinOp::Eq,
                Some(Tok::Ne) => BinOp::Ne,
                _ => break,
            };
            self.advance();
            let right = self.term()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut left = self.factor()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => BinOp::Add,
                Some(Tok::Minus) => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.factor()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn factor(&mut self) -> Result<Expr, String> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => BinOp::Mul,
                Some(Tok::Slash) => BinOp::Div,
                Some(Tok::Percent) => BinOp::Rem,
                _ => break,
            };
            self.advance();
            let right = self.unary()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Tok::Minus) => {
                self.advance();
                Ok(Expr::Unary(UnOp::Neg, Box::new(self.unary()?)))
            }
            Some(Tok::Bang) => {
                self.advance();
                Ok(Expr::Unary(UnOp::Not, Box::new(self.unary()?)))
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.advance() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::LParen) => {
                let e = self.expr()?;
                self.expect(&Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::Ident(name)) => match name.as_str() {
                "true" => Ok(Expr::Num(1.0)),
                "false" => Ok(Expr::Num(0.0)),
                "self" => {
                    self.expect(&Tok::Dot)?;
                    let channel = self.channel()?;
                    self.expect(&Tok::Dot)?;
                    let comp = self.axis()?;
                    Ok(Expr::Field(channel, comp))
                }
                _ if self.peek() == Some(&Tok::LParen) => {
                    self.advance();
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.expr()?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(&Tok::RParen)?;
                    Ok(Expr::Call(name, args))
                }
                _ => Ok(Expr::Var(name)),
            },
            other => Err(format!("unexpected token {other:?} in expression")),
        }
    }
}

// ---------------------------------------------------------------------------
// Evaluator
// ---------------------------------------------------------------------------

/// Per-frame evaluation context: the entity's transform plus the readonly time bindings and
/// any `let`-bound variables. Rotation is edited via Euler angles, written back on `finish`.
pub struct EvalCtx<'a> {
    transform: &'a mut Transform,
    time: f32,
    dt: f32,
    vars: HashMap<String, f32>,
    euler: Vec3,
    euler_dirty: bool,
}

impl<'a> EvalCtx<'a> {
    /// Build a context over `transform`, snapshotting its rotation as Euler angles.
    pub fn new(transform: &'a mut Transform, time: f32, dt: f32) -> Self {
        let (y, x, z) = transform.rotation.to_euler(EulerRot::YXZ);
        Self {
            transform,
            time,
            dt,
            vars: HashMap::new(),
            euler: Vec3::new(x, y, z),
            euler_dirty: false,
        }
    }

    fn finish(&mut self) {
        if self.euler_dirty {
            self.transform.rotation =
                Quat::from_euler(EulerRot::YXZ, self.euler.y, self.euler.x, self.euler.z);
            self.euler_dirty = false;
        }
    }
}

/// Evaluate a parsed `program` against `ctx`, applying it to the transform.
pub fn evaluate(program: &Program, ctx: &mut EvalCtx) -> Result<(), String> {
    for stmt in &program.stmts {
        exec(stmt, ctx)?;
    }
    ctx.finish();
    Ok(())
}

fn exec(stmt: &Stmt, ctx: &mut EvalCtx) -> Result<(), String> {
    match stmt {
        Stmt::Let(name, e) => {
            let v = eval(e, ctx)?;
            ctx.vars.insert(name.clone(), v);
        }
        Stmt::Assign(lvalue, e) => {
            let v = eval(e, ctx)?;
            match lvalue {
                LValue::Var(name) => {
                    ctx.vars.insert(name.clone(), v);
                }
                LValue::Field(Channel::Position, c) => ctx.transform.translation[*c] = v,
                LValue::Field(Channel::Scale, c) => ctx.transform.scale[*c] = v,
                LValue::Field(Channel::Rotation, c) => {
                    ctx.euler[*c] = v;
                    ctx.euler_dirty = true;
                }
                LValue::ScaleUniform => ctx.transform.scale = Vec3::splat(v),
            }
        }
        Stmt::If(cond, then, els) => {
            let branch = if eval(cond, ctx)? != 0.0 { then } else { els };
            for s in branch {
                exec(s, ctx)?;
            }
        }
        Stmt::Spin(e) => {
            ctx.euler.y += eval(e, ctx)? * ctx.dt;
            ctx.euler_dirty = true;
        }
        Stmt::Rotate(axis, e) => {
            ctx.euler[*axis] += eval(e, ctx)? * ctx.dt;
            ctx.euler_dirty = true;
        }
        Stmt::Translate(x, y, z) => {
            let v = Vec3::new(eval(x, ctx)?, eval(y, ctx)?, eval(z, ctx)?);
            ctx.transform.translation += v * ctx.dt;
        }
        Stmt::ScaleCmd(e) => {
            ctx.transform.scale = Vec3::splat(eval(e, ctx)?);
        }
    }
    Ok(())
}

fn eval(expr: &Expr, ctx: &EvalCtx) -> Result<f32, String> {
    match expr {
        Expr::Num(n) => Ok(*n),
        Expr::Var(name) => match name.as_str() {
            "time" => Ok(ctx.time),
            "dt" => Ok(ctx.dt),
            "pi" => Ok(core::f32::consts::PI),
            _ => ctx
                .vars
                .get(name)
                .copied()
                .ok_or_else(|| format!("unknown variable `{name}`")),
        },
        Expr::Field(channel, c) => Ok(match channel {
            Channel::Position => ctx.transform.translation[*c],
            Channel::Scale => ctx.transform.scale[*c],
            Channel::Rotation => ctx.euler[*c],
        }),
        Expr::Unary(op, inner) => {
            let v = eval(inner, ctx)?;
            Ok(match op {
                UnOp::Neg => -v,
                UnOp::Not => f32::from(v == 0.0),
            })
        }
        Expr::Bin(op, l, r) => {
            let a = eval(l, ctx)?;
            let b = eval(r, ctx)?;
            Ok(match op {
                BinOp::Add => a + b,
                BinOp::Sub => a - b,
                BinOp::Mul => a * b,
                BinOp::Div => a / b,
                BinOp::Rem => a % b,
                BinOp::Lt => f32::from(a < b),
                BinOp::Le => f32::from(a <= b),
                BinOp::Gt => f32::from(a > b),
                BinOp::Ge => f32::from(a >= b),
                BinOp::Eq => f32::from(a == b),
                BinOp::Ne => f32::from(a != b),
            })
        }
        Expr::Call(name, args) => call(name, args, ctx),
    }
}

fn call(name: &str, args: &[Expr], ctx: &EvalCtx) -> Result<f32, String> {
    let arg = |i: usize| eval(&args[i], ctx);
    let arity = |n: usize| {
        if args.len() == n {
            Ok(())
        } else {
            Err(format!(
                "`{name}` expects {n} argument(s), got {}",
                args.len()
            ))
        }
    };
    match name {
        "sin" => {
            arity(1)?;
            Ok(ops::sin(arg(0)?))
        }
        "cos" => {
            arity(1)?;
            Ok(ops::cos(arg(0)?))
        }
        "tan" => {
            arity(1)?;
            Ok(ops::tan(arg(0)?))
        }
        "abs" => {
            arity(1)?;
            Ok(arg(0)?.abs())
        }
        "sqrt" => {
            arity(1)?;
            Ok(arg(0)?.sqrt())
        }
        "floor" => {
            arity(1)?;
            Ok(arg(0)?.floor())
        }
        "sign" => {
            arity(1)?;
            Ok(arg(0)?.signum())
        }
        "min" => {
            arity(2)?;
            Ok(arg(0)?.min(arg(1)?))
        }
        "max" => {
            arity(2)?;
            Ok(arg(0)?.max(arg(1)?))
        }
        other => Err(format!("unknown function `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::{evaluate, parse, EvalCtx};
    use bevy_math::{EulerRot, Vec3};
    use bevy_transform::components::Transform;

    fn run(src: &str, time: f32, dt: f32) -> Result<Transform, String> {
        let program = parse(src)?;
        let mut t = Transform::default();
        let mut ctx = EvalCtx::new(&mut t, time, dt);
        evaluate(&program, &mut ctx)?;
        Ok(t)
    }

    #[test]
    fn arithmetic_assignment() {
        let t = run("self.position.y = 2 + 3 * 4", 0.0, 0.0).unwrap();
        assert!((t.translation.y - 14.0).abs() < 1e-6);
    }

    #[test]
    fn let_bindings_and_uniform_scale() {
        let t = run("let x = 4; self.scale = x / 2", 0.0, 0.0).unwrap();
        assert_eq!(t.scale, Vec3::splat(2.0));
    }

    #[test]
    fn conditionals() {
        let hot = run(
            "if time > 1 { self.position.x = 10 } else { self.position.x = -1 }",
            2.0,
            0.0,
        )
        .unwrap();
        assert!((hot.translation.x - 10.0).abs() < 1e-6);
        let cold = run(
            "if time > 1 { self.position.x = 10 } else { self.position.x = -1 }",
            0.0,
            0.0,
        )
        .unwrap();
        assert!((cold.translation.x + 1.0).abs() < 1e-6);
    }

    #[test]
    fn functions_and_pi() {
        let t = run(
            "self.position.x = max(min(5, 3), 1); self.position.y = sin(0)",
            0.0,
            0.0,
        )
        .unwrap();
        assert!((t.translation.x - 3.0).abs() < 1e-6);
        assert!(t.translation.y.abs() < 1e-6);
    }

    #[test]
    fn legacy_spin_rotates_about_y() {
        let t = run("spin 2.0", 0.0, 0.5).unwrap();
        let (y, _, _) = t.rotation.to_euler(EulerRot::YXZ);
        assert!((y - 1.0).abs() < 1e-4, "spin 2 * dt 0.5 = 1 radian about Y");
    }

    #[test]
    fn read_modify_rotation_channel() {
        let t = run("self.rotation.y = self.rotation.y + 0.5", 0.0, 0.0).unwrap();
        let (y, _, _) = t.rotation.to_euler(EulerRot::YXZ);
        assert!((y - 0.5).abs() < 1e-4);
    }

    #[test]
    fn newline_separated_statements() {
        let t = run("self.position.x = 1\nself.position.z = 2\n", 0.0, 0.0).unwrap();
        assert!((t.translation.x - 1.0).abs() < 1e-6);
        assert!((t.translation.z - 2.0).abs() < 1e-6);
    }

    #[test]
    fn parse_errors_are_reported_not_panicked() {
        assert!(parse("let = 5").is_err());
        assert!(parse("self.position.q = 1").is_err());
        assert!(parse("if x { ").is_err());
        assert!(run("self.position.x = nope", 0.0, 0.0).is_err());
        assert!(run("self.position.x = sin(1, 2)", 0.0, 0.0).is_err());
    }

    #[test]
    fn comments_are_ignored() {
        let t = run("# set x\nself.position.x = 5 # trailing", 0.0, 0.0).unwrap();
        assert!((t.translation.x - 5.0).abs() < 1e-6);
    }
}
