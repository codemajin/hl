//! Bundles the syntaxes of two-face with fixes for hangs in JavaScript and
//! Scala.
//!
//! Both hang the same way: two contexts switch to each other with `set`
//! without consuming anything, which syntect does not catch (it only stops
//! a push followed by a pop that consume nothing), so it loops forever.
//!
//! In JavaScript, the end of an expression statement looks ahead for a line
//! that ends with only blanks and comments, and for a line continuation such
//! as a binary operator. The continuation also matches the `/` of a comment,
//! so `x /*` sets one context after the other. HTML, Markdown and the other
//! syntaxes that embed JavaScript hang the same way. The fix keeps the
//! continuation from matching the start of a comment.
//!
//! In Scala, a type looks ahead for `@` and an identifier to read an
//! annotation, but the identifier may be an operator, while an annotation
//! takes only an alphanumeric one. So on `A @> B` or `:a@<` nothing reads
//! the `@`, and the type sets itself again. The fix looks ahead for the
//! identifiers an annotation takes only, so that `@>` is left to the rest
//! of the syntax as an operator.
//!
//! Remove a fix once the definitions are fixed upstream.

use std::{env, path::PathBuf};

use syntect::{
    dumps,
    parsing::{
        Regex, SyntaxDefinition, SyntaxSetBuilder,
        syntax_definition::{MatchPattern, Pattern},
    },
};

/// The scope of the JavaScript syntax. CFML Script has the same lookahead
/// but does not hang, so it is left alone.
const JAVASCRIPT: &str = "source.js";
/// The operators that continue an expression on the next line.
const CONTINUATION: &str = r"[-+*/%><=&|^\[(;,.:?]";
/// The same operators, without `/` that starts a comment.
const FIXED_CONTINUATION: &str = r"(?:[-+*%><=&|^\[(;,.:?]|/(?![/*]))";

/// The scope of the Scala syntax.
const SCALA: &str = "source.scala";
/// The start of a lookahead for an annotation, `(?=@{{plainid}})`.
const ANNOTATION_LOOKAHEAD: &str = "(?=@";
/// The start and end of the annotation itself, `(@)({{alphaplainid}})`,
/// which tell the expanded `alphaplainid`.
const ANNOTATION: (&str, &str) = ("(@)(", ")");
/// The end of the annotation with a qualified name, `(@)({{alphaplainid}})(\.)`.
const QUALIFIED: &str = r"(\.)";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let mut builder = SyntaxSetBuilder::new();
    let (mut javascript, mut scala) = (0, 0);
    let definitions = two_face::syntax::extra_newlines()
        .into_builder()
        .syntaxes()
        .to_vec();
    for mut syntax in definitions {
        match syntax.scope.build_string().as_str() {
            JAVASCRIPT => javascript += fix_javascript(&mut syntax),
            SCALA => scala += fix_scala(&mut syntax),
            _ => {}
        }
        builder.add(syntax);
    }
    assert!(
        javascript > 0,
        "the line continuation of two-face's JavaScript has changed: \
         check whether `x /*` still hangs and update or remove build.rs"
    );
    assert!(
        scala > 0,
        "the annotation lookahead of two-face's Scala has changed: \
         check whether `:a@<` still hangs and update or remove build.rs"
    );

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    dumps::dump_to_uncompressed_file(&builder.build(), out.join("syntaxes.bin"))
        .expect("cannot write the syntax dump");
}

/// The match patterns of all contexts of `syntax`.
fn match_patterns(syntax: &mut SyntaxDefinition) -> impl Iterator<Item = &mut MatchPattern> {
    syntax.contexts.values_mut().flat_map(|context| {
        context
            .patterns
            .iter_mut()
            .filter_map(|pattern| match pattern {
                Pattern::Match(m) => Some(m),
                Pattern::Include(_) => None,
            })
    })
}

/// Keeps the line continuation from matching the start of a comment, and
/// returns how many patterns it changed.
fn fix_javascript(syntax: &mut SyntaxDefinition) -> usize {
    let mut fixed = 0;
    for m in match_patterns(syntax) {
        let regex = m.regex.regex_str();
        if regex.contains(CONTINUATION) {
            m.regex = Regex::new(regex.replace(CONTINUATION, FIXED_CONTINUATION));
            fixed += 1;
        }
    }
    fixed
}

/// Narrows the lookaheads for an annotation to the identifiers an
/// annotation takes, and returns how many it changed. Panics if a lookahead
/// is left that it does not know.
fn fix_scala(syntax: &mut SyntaxDefinition) -> usize {
    // Variables are expanded as text, so `(?=@{{plainid}})` reads
    // `(?=@(?:ALPHA|OP))`, where ALPHA is the expanded `alphaplainid`.
    let alpha = match_patterns(syntax)
        .find_map(|m| {
            let regex = m.regex.regex_str();
            // `(@)({{alphaplainid}})(\.)` starts a qualified name instead.
            if regex.ends_with(QUALIFIED) {
                return None;
            }
            let alpha = regex
                .strip_prefix(ANNOTATION.0)?
                .strip_suffix(ANNOTATION.1)?;
            Some(alpha.to_owned())
        })
        .expect("the Scala syntax has the annotation `(@)({{alphaplainid}})`");
    let plainid = format!("{ANNOTATION_LOOKAHEAD}(?:{alpha}|");
    let fixed_regex = format!("{ANNOTATION_LOOKAHEAD}{alpha})");

    let mut fixed = 0;
    for m in match_patterns(syntax) {
        let regex = m.regex.regex_str();
        if regex.starts_with(&plainid) {
            m.regex = Regex::new(fixed_regex.clone());
            fixed += 1;
        } else {
            assert!(
                !regex.starts_with(ANNOTATION_LOOKAHEAD) || regex == fixed_regex,
                "an unknown annotation lookahead in two-face's Scala: {regex}"
            );
        }
    }
    fixed
}
