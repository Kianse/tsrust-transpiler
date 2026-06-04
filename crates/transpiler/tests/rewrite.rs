#[cfg(test)]
mod tests {
    use tsrust_transpiler::transpile_str;

    fn t(src: &str) -> String {
        transpile_str(src, "test").unwrap()
    }

    #[test]
    fn exported_arrow_becomes_pub_fn() {
        let src = r#"
            export const add = (a: i32, b: i32): i32 => (a + b)
            export const ping = (n: i32): () => { println!("{}", n); }
        "#;
        let out = t(src);
        assert!(out.contains("pub fn add(a: i32, b: i32) -> i32 { a + b }"));
        assert!(out.contains(r#"pub fn ping(n: i32) -> () { println!("{}", n); }"#));
    }

    #[test]
    fn local_arrow_becomes_closure() {
        let src = r#"
            function demo(): i32 {
                let base = 3;
                let f = (x: i32): i32 => (x + base);
                ( f(4) )
            }
        "#;
        let out = t(src);
        assert!(out.contains("let f = |x: i32| -> i32 { x + base };"));
        assert!(out.contains("fn demo() -> i32 {"));
    }

    #[test]
    fn move_arrow_kept() {
        let src = r#"
            function g(): i32 {
                let base = 5;
                let f = move (x: i32): i32 => { x + base };
                ( f(7) )
            }
        "#;
        let out = t(src);
        assert!(out.contains("let f = move |x: i32| -> i32 { x + base };"));
    }

    #[test]
    fn hyphenated_crate_names_are_underscored() {
        let out = t(r#"
            import { Value } from "serde-json.core";
            export { Thing } from "../my-crate.utils";
        "#);
        assert!(out.contains("use serde_json::core::Value;"));
        assert!(out.contains("pub use crate::super::my_crate::utils::{Thing};"));
    }

    #[test]
    fn trims_extra_dots_and_slashes() {
        let out = t(r#"
            import { foo } from "./api./v1..users";
        "#);
        // We just ensure it didn't blow up and created a sensible path:
        assert!(out.contains("use crate::api::v1::users::foo;"));
    }

    #[test]
    fn export_star_from() {
        let out = t(r#"
            export * from "./prelude";
        "#);
        assert!(out.contains("pub use crate::prelude::*;"));
    }

    #[test]
    fn nested_import_group() {
        let out = t(r#"
            import { foo, bar::{Baz, qux as q} } from "x";
        "#);
        assert!(out.contains("use x::foo;"));
        assert!(out.contains("use x::bar::{Baz, qux as q};"));
    }

    #[test]
    fn dot_paths_absolute_and_namespace() {
        let out = t(r#"
            import { HashMap } from "std.collections";
            import * as io from "std.io";
        "#);
        assert!(out.contains("use std::collections::HashMap;"));
        assert!(out.contains("use std::io as io;"));
    }

    #[test]
    fn dot_paths_relative() {
        let out = t(r#"
            import { double } from "./util.math";
            export { User } from "../models.user";
        "#);
        assert!(out.contains("use crate::util::math::double;"));
        assert!(out.contains("pub use crate::super::models::user::{User};"));
    }

    #[test]
    fn non_unit_without_terminal_errors() {
        let src = r#"
            function bad(a: i32): i32 {
                let x = a + 1;
                // missing return marker here
            }
        "#;
        let err = transpile_str(src, "test").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("must end with `return EXPR;` or `(EXPR)`"));
    }

    #[test]
    fn export_fn_and_return_tail() {
        let src = r#"
            export function add(a: i32, b: i32): i32 { (a + b) }
            function ping(): () { println!("hi"); }
            function ret_unit_sugar(): () { return; }
        "#;
        let out = t(src);
        assert!(out.contains("pub fn add"));
        assert!(out.contains("-> i32 { a + b }"));
        assert!(out.contains("fn ping()"));
        assert!(out.contains(r#"println!("hi");"#));
        assert!(out.contains("return ;"));
    }

    #[test]
    fn for_of_and_imports() {
        let src = r#"
            import { HashMap } from "std.collections";
            import * as io from "std.io";
            import { foo as bar } from "./util";

            function f(): () {
              for (x of xs) { println!("{}", x); }
            }
        "#;
        let out = t(src);
        assert!(out.contains("use std::collections::HashMap;"));
        assert!(out.contains("use std::io as io;"));
        assert!(out.contains("use crate::util::foo as bar;"));
        assert!(out.contains("for x in xs"));
    }

    #[test]
    fn export_from_and_reexport() {
        let src = r#"
            export { Foo, Bar as Baz } from "./models";
            export { run };
        "#;
        let out = t(src);
        assert!(out.contains("pub use crate::models::{Foo, Bar as Baz};"));
        assert!(out.contains("pub use self::{run};"));
    }

    #[test]
    fn paren_tail_with_nested_parens_is_stripped() {
        let src = r#"
            function g(): i32 {
                ( double(add(a, b)) )
            }
        "#;
        let out = t(src);
        // No outer parens around the tail:
        assert!(out.contains("-> i32 {"));
        assert!(out.contains("double(add(a, b))"));
        assert!(!out.contains("( double(add(a, b)) )"));
    }

    #[test]
    fn error_has_file_line_col() {
        let src = "function bad(): i32 { let x = 1; }";
        let err = transpile_str(src, "src/main.tsrust").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("src/main.tsrust:"));
        assert!(msg.contains("Non-unit function 'bad'"));
    }

    #[test]
    fn coalesce_basic_and_chain() {
        let src = r#"
            function f(a: i32?): i32 {
                let x = a ?? 10;
                let y = a ?? 1 ?? 2;
                (x + y)
            }
        "#;
        let out = t(src);
        assert!(out.contains("a: Option<i32>"));
        assert!(out.contains("let x = (a).unwrap_or(10);"));
        assert!(out.contains("let y = ((a).unwrap_or(1)).unwrap_or(2);"));
    }

    #[test]
    fn optional_chaining_props_and_calls() {
        let src = r#"
            function g(user: User?): i32? {
                let n = user?.name?.len();
                (n)
            }
        "#;
        let out = t(src);
        assert!(out.contains(".as_ref().map(|__v| __v.name)"));
        assert!(out.contains(".map(|__v| __v.len())") || out.contains(".map(|__v| __v.len())"));
    }

    #[test]
    fn optional_and_default_params() {
        let src = r#"
            export function add(a: i32 = 1, b?: i32): i32 {
                ( (a ?? 0) + (b ?? 0) )
            }
        "#;
        let out = t(src);
        // signature
        assert!(out.contains("pub fn add(a: Option<i32>, b: Option<i32>) -> i32"));
        // injected default
        assert!(out.contains("let a = a.unwrap_or(1);"));
        // coalescing on b stays as unwrap_or
        assert!(out.contains("(b).unwrap_or(0)"));
    }

    #[test]
    fn switch_basic_cases_and_default() {
        let src = r#"
            function f(n: i32): i32 {
                switch (n) {
                    case 0: { (10) } break;
                    case 1:
                    case 2:
                        { (20) }
                        break;
                    default:
                        { (99) }
                }
            }
        "#;
        let out = t(src);
        assert!(out.contains("match n {"));
        // 0 => { 10 }
        assert!(out.contains("0 => {"));
        assert!(out.contains("10"));
        // 1 | 2 => { 20 }
        assert!(out.contains("1 | 2 => {"));
        assert!(out.contains("20"));
        // _ => { 99 }
        assert!(out.contains("_ => {"));
        assert!(out.contains("99"));
        assert!(out.contains("fn f(n: i32) -> i32"));
    }

    #[test]
    fn switch_nested_blocks_and_breaks_removed() {
        let src = r#"
            function g(x: i32): () {
                switch (x) {
                    case 7:
                        { if (x > 0) { println!("pos"); } }
                        break;
                    default:
                        { println!("other"); }
                        break;
                }
            }
        "#;
        let out = t(src);
        // no literal "break;" left inside arms
        assert!(!out.contains("break;"));
        assert!(out.contains("match x {"));
        assert!(out.contains("_ => {"));
    }


    #[test]
    fn return_wrapped_parens_are_stripped() {
        let src = r#"
            function f(a: i32): i32 {
                let times = (x: i32): i32 => { x + 1 };
                return(times(a))
            }
        "#;
        let out = t(src);
        assert!(out.contains("return times(a)"));
    }

}
