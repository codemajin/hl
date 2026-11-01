//! Bundles the syntaxes of two-face with a fix for a hang in JavaScript.
//!
//! In two-face's JavaScript, the end of an expression statement looks ahead
//! for a line that ends with only blanks and comments, and for a line
//! continuation such as a binary operator. Both switch contexts with `set`
//! without consuming anything, and the continuation also matches the `/` of
//! a comment, so `x /*` sets one context after the other forever: syntect
//! only catches such loops between a push and a pop. HTML, Markdown and the
//! other syntaxes that embed JavaScript hang the same way.
//!
//! The fix keeps the continuation from matching the start of a comment.
//! Remove this script once the definitions are fixed upstream.

use std::{env, path::PathBuf};

use syntect::{
    dumps,
    parsing::{Regex, SyntaxSetBuilder, syntax_definition::Pattern},
};

/// The scope of the syntax to fix. CFML Script has the same lookahead but
/// does not hang, so it is left alone.
const JAVASCRIPT: &str = "source.js";
/// The operators that continue an expression on the next line.
const CONTINUATION: &str = r"[-+*/%><=&|^\[(;,.:?]";
/// The same operators, without `/` that starts a comment.
const FIXED_CONTINUATION: &str = r"(?:[-+*%><=&|^\[(;,.:?]|/(?![/*]))";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let definitions = two_face::syntax::extra_newlines()
        .into_builder()
        .syntaxes()
        .to_vec();
    let mut builder = SyntaxSetBuilder::new();
    let mut fixed = 0;
    for mut syntax in definitions {
        let is_javascript = syntax.scope.build_string() == JAVASCRIPT;
        for context in syntax.contexts.values_mut() {
            for pattern in &mut context.patterns {
                let Pattern::Match(m) = pattern else { continue };
                let regex = m.regex.regex_str();
                if is_javascript && regex.contains(CONTINUATION) {
                    m.regex = Regex::new(regex.replace(CONTINUATION, FIXED_CONTINUATION));
                    fixed += 1;
                }
            }
        }
        builder.add(syntax);
    }
    assert!(
        fixed > 0,
        "the line continuation of two-face's JavaScript has changed: \
         check whether `x /*` still hangs and update or remove build.rs"
    );

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    dumps::dump_to_uncompressed_file(&builder.build(), out.join("syntaxes.bin"))
        .expect("cannot write the syntax dump");
}
