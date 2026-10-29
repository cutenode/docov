use oxc_span::SourceType;

use super::*;

/// `(name, kind, documented)` for each item `source` has, as TypeScript.
fn items(source: &str, scope: Scope) -> Vec<(String, Kind, bool)> {
    scan(source, SourceType::ts(), scope).unwrap().into_iter().map(|i| (i.name, i.kind, i.documented)).collect()
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
    let items = scan(source, SourceType::ts(), Scope::All).unwrap();
    assert_eq!((items[0].line, items[0].column), (1, 1));
    assert_eq!((items[1].line, items[1].column), (3, 3));
}

#[test]
fn syntax_errors_fail_the_scan() {
    let error = scan("export function (", SourceType::ts(), Scope::All).unwrap_err();
    assert!(error.offset > 0, "{error:?}");
}

#[test]
fn files_are_read_by_their_extension() {
    let dir = tempfile::tempdir().unwrap();
    let jsx = dir.path().join("a.jsx");
    fs::write(&jsx, "/** A */\nexport const A = () => <div />;\n").unwrap();
    assert_eq!(scan_file(&jsx, Scope::Exports).unwrap().totals(), Totals { documented: 1, total: 1 });

    let broken = dir.path().join("b.ts");
    fs::write(&broken, "\n  export function (").unwrap();
    let error = scan_file(&broken, Scope::Exports).unwrap_err().to_string();
    assert!(error.starts_with(&format!("{}:2:", broken.display())), "{error}");

    let text = dir.path().join("c.txt");
    assert!(matches!(scan_file(&text, Scope::Exports), Err(Error::Language { .. })));
    let missing = dir.path().join("missing.ts");
    assert!(matches!(scan_file(&missing, Scope::Exports), Err(Error::Read { .. })));
}
