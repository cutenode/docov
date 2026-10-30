use oxc_span::SourceType;

use super::*;
use crate::{
    Scope,
    feature::{access::Access, exports::Exports, inherit::Inherit},
};

/// `(name, kind, documented)` for each item `source` has, as TypeScript.
fn items(source: &str, scope: Scope) -> Vec<(String, Kind, bool)> {
    scan(source, SourceType::ts(), &scope.into()).unwrap().into_iter().map(|i| (i.name, i.kind, i.documented)).collect()
}

fn item(name: &str, kind: Kind, documented: bool) -> (String, Kind, bool) {
    (name.to_owned(), kind, documented)
}

#[test]
fn exported_declarations_count_and_their_docs_are_found() {
    let source = "
/** a */
export function a() {}
export function b() {}
/** C */
export class C {}
export const d = 1, e = 2;
/** I */
export interface I {}
/** T */
export type T = string;
export enum E { A }
/** N */
export namespace N {}
function local() {}
";
    assert_eq!(
        items(source, Scope::Exports),
        [
            item("a", Kind::Function, true),
            item("b", Kind::Function, false),
            item("C", Kind::Class, true),
            item("d", Kind::Variable, false),
            item("e", Kind::Variable, false),
            item("I", Kind::Interface, true),
            item("T", Kind::Type, true),
            item("E", Kind::Enum, false),
            item("N", Kind::Namespace, true),
        ]
    );
    assert_eq!(items(source, Scope::All).last(), Some(&item("local", Kind::Function, false)));
}

#[test]
fn only_jsdoc_with_something_in_it_counts() {
    let source = "
// line
export function a() {}
/* block */
export function b() {}
/** */
export function c() {}
/**
 *
 */
export function d() {}
/** @license MIT */
export function e() {}
/** @internal */
export function f() {}
";
    let documented: Vec<bool> = items(source, Scope::Exports).into_iter().map(|(.., d)| d).collect();
    assert_eq!(documented, [false, false, false, false, false, true]);
}

#[test]
fn names_exported_apart_from_their_declarations_count() {
    let source = "
/** a */
function a() {}
function b() {}
class C { m() {} }
const d = 1;
function hidden() {}
export { a, b as renamed };
export default C;
export = d;
export { x } from './x';
export * from './y';
";
    assert_eq!(
        items(source, Scope::Exports),
        [
            item("a", Kind::Function, true),
            item("b", Kind::Function, false),
            item("C", Kind::Class, false),
            item("C.m", Kind::Method, false),
            item("d", Kind::Variable, false),
        ]
    );
}

#[test]
fn commonjs_exports_count() {
    let source = "
const fs = require('fs');
/** a */
function a() {}
function hidden() {}
/** b */
exports.b = function () {};
module.exports.c = 1;
exports.a = a;
module.exports = {
    /** d */
    d() {},
    e: () => 1,
    f: 1,
    a,
    ...rest,
};
module.exports = class G {};
/** default */
module.exports = 1;
window.x = 1;
other.exports.y = 1;
exports[key] = 1;
x = 1;
";
    assert_eq!(
        items(source, Scope::Exports),
        [
            item("a", Kind::Function, true),
            item("b", Kind::Function, true),
            item("c", Kind::Variable, false),
            item("d", Kind::Function, true),
            item("e", Kind::Function, false),
            item("f", Kind::Variable, false),
            item("G", Kind::Class, false),
            item("default", Kind::Default, true),
        ]
    );
    assert_eq!(items(source, Scope::All).len(), 9, "hidden, but not the require()");
}

#[test]
fn default_exports_count() {
    assert_eq!(
        items("/** f */ export default function () {}", Scope::Exports),
        [item("default", Kind::Function, true)]
    );
    assert_eq!(items("export default class Named {}", Scope::Exports), [item("Named", Kind::Class, false)]);
    assert_eq!(items("export default interface I { a: 1 }", Scope::Exports).len(), 2);
    assert_eq!(items("/** x */ export default { a: 1 };", Scope::Exports), [item("default", Kind::Default, true)]);
}

#[test]
fn public_class_members_count() {
    let source = "
export class C {
    /** ignored: constructors are documented by their class */
    constructor() {}
    /** m */
    m() {}
    #hidden() {}
    private secret = 1;
    protected shared = 2;
    static s = 3;
    /** a value */
    get value() { return 1; }
    set value(v) {}
    set onlySet(v) {}
    /** accessor */
    accessor auto = 1;
    [Symbol.iterator]() {}
    'quoted'() {}
}
";
    assert_eq!(
        items(source, Scope::Exports),
        [
            item("C", Kind::Class, false),
            item("C.m", Kind::Method, true),
            item("C.shared", Kind::Property, false),
            item("C.s", Kind::Property, false),
            item("C.value", Kind::Accessor, true),
            item("C.onlySet", Kind::Accessor, false),
            item("C.auto", Kind::Accessor, true),
            item("C.[Symbol.iterator]", Kind::Method, false),
            item("C.quoted", Kind::Method, false),
        ]
    );
}

#[test]
fn interface_members_count() {
    let source = "
export interface I {
    /** a */
    a: string;
    b(): void;
    [key: string]: unknown;
    (): void;
}
";
    assert_eq!(
        items(source, Scope::Exports),
        [item("I", Kind::Interface, false), item("I.a", Kind::Property, true), item("I.b", Kind::Method, false)]
    );
}

#[test]
fn overload_signatures_count_and_their_implementation_does_not() {
    let source = "
/** one */
export function f(a: string): void;
export function f(a: number): void;
export function f(a: unknown) {}
export function g() {}
export class C {
    /** m */
    m(a: string): void;
    m(a: unknown) {}
    n() {}
}
declare function h(): void;
declare function h(a: string): void;
";
    assert_eq!(
        items(source, Scope::All),
        [
            item("f", Kind::Function, true),
            item("f", Kind::Function, false),
            item("g", Kind::Function, false),
            item("C", Kind::Class, false),
            item("C.m", Kind::Method, true),
            item("C.n", Kind::Method, false),
            item("h", Kind::Function, false),
            item("h", Kind::Function, false),
        ]
    );
}

#[test]
fn decorated_classes_find_their_docs() {
    let source = "
/** before */
@sealed
export class A {}
/** after */
export @sealed class B {}
export class C {
    /** member */
    @log m() {}
}
";
    let documented: Vec<bool> = items(source, Scope::Exports).into_iter().map(|(.., d)| d).collect();
    assert_eq!(documented, [true, true, false, true]);
}

#[test]
fn ambient_modules_and_destructuring_have_nothing_to_count() {
    let source = "
declare module 'x' { export function f(): void; }
declare global { interface Window { a: 1 } }
import fs = require('fs');
export = { a: 1 };
go();
export const { a, b } = { a: 1, b: 2 };
";
    assert_eq!(items(source, Scope::All), []);
}

#[test]
fn items_point_at_the_line_and_column_their_doc_goes_on() {
    let source = "const é = 1;\n\n  export function f() {}\n";
    let items = scan(source, SourceType::ts(), &Scope::All.into()).unwrap();
    assert_eq!((items[0].line, items[0].column), (1, 1));
    assert_eq!((items[1].line, items[1].column), (3, 3));
}

#[test]
fn syntax_errors_fail_the_scan() {
    let error = scan("export function (", SourceType::ts(), &Scope::All.into()).unwrap_err();
    assert!(error.offset > 0, "{error:?}");
}

#[test]
fn files_are_read_by_their_extension() {
    let dir = tempfile::tempdir().unwrap();
    let jsx = dir.path().join("a.jsx");
    fs::write(&jsx, "/** A */\nexport const A = () => <div />;\n").unwrap();
    assert_eq!(scan_file(&jsx, &Scope::Exports.into()).unwrap().totals(), Totals { documented: 1, total: 1 });

    let broken = dir.path().join("b.ts");
    fs::write(&broken, "\n  export function (").unwrap();
    let error = scan_file(&broken, &Scope::Exports.into()).unwrap_err().to_string();
    assert!(error.starts_with(&format!("{}:2:", broken.display())), "{error}");

    let text = dir.path().join("c.txt");
    assert!(matches!(scan_file(&text, &Scope::Exports.into()), Err(Error::Language { .. })));
    let missing = dir.path().join("missing.ts");
    assert!(matches!(scan_file(&missing, &Scope::Exports.into()), Err(Error::Read { .. })));
}

#[test]
fn require_description_leaves_out_blocks_with_tags_alone() {
    let source = "
/** @type {string} */
export const a = '';
/** @deprecated */
export function b() {}
/** Docs. @internal */
export function c() {}
/**
 * @summary Docs.
 */
export function d() {}
/** @description */
export function e() {}
/** @classdesc Docs. */
export class F {
    /** @classdesc Docs. */
    g() {}
    /** @inheritDoc */
    h() {}
}
";
    let rules = Rules { require_description: true, ..Rules::default() };
    let documented: Vec<bool> =
        scan(source, SourceType::ts(), &rules).unwrap().into_iter().map(|i| i.documented).collect();
    assert_eq!(documented, [false, false, true, true, false, true, false, true]);
    assert!(items(source, Scope::Exports).iter().all(|(.., documented)| *documented));
}

#[test]
fn excluded_tags_take_their_items_out() {
    let source = "
/** @internal */
export const a = 1;
/** Mentions @internal, which isn't a tag here. */
export const b = 1;
/**
 * C.
 * @hidden
 */
export const c = 1;
export const d = 1;
";
    let rules = Rules { exclude_tags: vec!["internal".into(), "hidden".into()], ..Rules::default() };
    let names: Vec<String> = scan(source, SourceType::ts(), &rules).unwrap().into_iter().map(|i| i.name).collect();
    assert_eq!(names, ["b", "d"]);
}

#[test]
fn ignore_comments_cover_the_next_line_or_the_file() {
    let source = "
// docov-ignore-next-line: legacy
export function a() {}
/** b */
// docov-ignore-next-line
export function b() {}
// docov-ignore-next-line
/** c */
export function c() {}
/* docov-ignore-next-line */
export class D { e() {} }
// docov-ignore-next-lines
export function f() {}
function g() {
    /* docov-ignore-file: not here */
}
";
    let items = scan(source, SourceType::ts(), &Rules::default()).unwrap();
    let ignored: Vec<(&str, bool, Option<&str>)> =
        items.iter().map(|i| (&*i.name, i.ignored(), i.ignore.as_ref().and_then(|i| i.reason.as_deref()))).collect();
    assert_eq!(
        ignored,
        [
            ("a", true, Some("legacy")),
            ("b", false, None),
            ("c", false, None),
            ("D", true, None),
            ("D.e", true, None),
            ("f", false, None),
        ]
    );
    // A documented item keeps its ignore, which it doesn't need.
    assert_eq!(items[1].ignore, Some(Ignore { reason: None, line: 5, column: 1, file: false }));
    assert_eq!(items[2].ignore, None, "the comment covers the JSDoc's line");

    let file = "export const a = 1;\n/** b */ export const b = 1;\nlet x; // docov-ignore-file : generated\n";
    let items = scan(file, SourceType::ts(), &Rules::default()).unwrap();
    let ignore = Ignore { reason: Some("generated".into()), line: 3, column: 8, file: true };
    assert_eq!([items[0].ignored(), items[1].ignored()], [true, false]);
    assert_eq!([&items[0].ignore, &items[1].ignore], [&Some(ignore.clone()), &Some(ignore)]);
}

#[test]
fn access_levels_pick_what_counts() {
    let source = "
/** @private */
export function a() {}
/** @access package */
export function b() {}
export class C {
    protected d = 1;
    /** @protected */
    e() {}
    /** @access public */
    protected f = 1;
}
";
    let names = |access: &[Access]| -> Vec<String> {
        let rules = Rules { access: Some(access.to_vec()), ..Rules::default() };
        scan(source, SourceType::ts(), &rules).unwrap().into_iter().map(|i| i.name).collect()
    };
    assert_eq!(items(source, Scope::Exports).len(), 6, "every level counts without access");
    assert_eq!(names(&[Access::Public]), ["C", "C.f"]);
    assert_eq!(names(&[Access::Protected, Access::Package]), ["b", "C.d", "C.e"]);
    assert_eq!(names(&[Access::Private]), ["a"]);
}

#[test]
fn override_members_inherit_their_docs_when_asked() {
    let source = "
export class A extends B {
    override m() {}
    override p = 1;
    override accessor q = 1;
    n() {}
    /** @inheritDoc */
    o() {}
}
";
    let documented = |inherit| {
        let rules = Rules { inherit, require_description: true, ..Rules::default() };
        scan(source, SourceType::ts(), &rules).unwrap().into_iter().map(|i| i.documented).collect::<Vec<_>>()
    };
    assert_eq!(documented(Inherit::Tag), [false, false, false, false, false, true]);
    assert_eq!(documented(Inherit::Override), [false, true, true, true, false, true]);
}

#[test]
fn export_mechanisms_can_be_turned_off_and_globals_on() {
    let source = "
export const a = 1;
const b = 1;
exports.b = b;
exports.c = 1;
window.e = 1;
const f = 1;
globalThis.f = f;
window.g.h = 1;
self.i = 1;
";
    let names = |scope, exports| -> Vec<String> {
        let rules = Rules { scope, exports, ..Rules::default() };
        scan(source, SourceType::ts(), &rules).unwrap().into_iter().map(|i| i.name).collect()
    };
    let on = Exports::default();
    assert_eq!(names(Scope::Exports, on), ["a", "b", "c"]);
    assert_eq!(names(Scope::Exports, Exports { esm: false, ..on }), ["b", "c"]);
    assert_eq!(names(Scope::Exports, Exports { cjs: false, ..on }), ["a"]);
    assert_eq!(names(Scope::Exports, Exports { globals: true, ..on }), ["a", "b", "c", "e", "f"]);
    assert_eq!(names(Scope::All, Exports { esm: false, cjs: false, ..on }), ["a", "b", "c", "f"]);
    assert_eq!(names(Scope::All, Exports { globals: true, ..on }), ["a", "b", "c", "e", "f"]);
}
