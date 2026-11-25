//! `pg_dump` output that sqlparser's `PostgreSqlDialect` rejects.
//!
//! [`rescue`] only sees statements the stock grammar failed on (see
//! `recovery`), so a statement the stock grammar accepts is never read here.
//! A rejected statement is either rewritten into a form the stock grammar
//! reads with the same meaning for the schema, or, when it only concerns
//! objects or settings the schema does not model, skipped with a warning.
//! Anything else stays a parse error: the forms below are an allowlist of
//! what `pg_dump` writes, not a blanket pass for unknown statements.

use sqlparser::ast::Statement;
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Span, Token, TokenWithSpan, Whitespace};

/// A rejected statement after its second reading.
#[derive(Debug)]
pub(crate) enum Rescued {
    /// The statement, read from a rewritten form.
    Statement(Box<Statement>),
    /// A statement the schema has no use for.
    Skipped {
        /// What was skipped, such as `CREATE PUBLICATION`.
        construct: String,
        /// The skipped statement.
        span: Span,
    },
}

/// Objects the schema does not model, as named after `CREATE`, `ALTER`, or
/// `DROP`, with each phrase listed before any shorter phrase it starts with.
///
/// `SCHEMA` and `DATABASE` are left out: `CREATE SCHEMA` can embed tables and
/// renaming a schema moves them. A `DROP` is only skipped without `CASCADE`
/// (`pg_dump --clean` never writes it), so it cannot reach modeled objects.
const UNMODELED_OBJECTS: &[&str] = &[
    "ACCESS METHOD",
    "AGGREGATE",
    "CAST",
    "COLLATION",
    "CONVERSION",
    "DEFAULT CONVERSION",
    "DOMAIN",
    "EVENT TRIGGER",
    "EXTENSION",
    "FOREIGN DATA WRAPPER",
    "FUNCTION",
    "LANGUAGE",
    "LARGE OBJECT",
    "OPERATOR CLASS",
    "OPERATOR FAMILY",
    "OPERATOR",
    "PROCEDURAL LANGUAGE",
    "PROCEDURE",
    "PUBLICATION",
    "ROLE",
    "ROUTINE",
    "SEQUENCE",
    "SERVER",
    "STATISTICS",
    "SUBSCRIPTION",
    "TABLESPACE",
    "TEXT SEARCH CONFIGURATION",
    "TEXT SEARCH DICTIONARY",
    "TEXT SEARCH PARSER",
    "TEXT SEARCH TEMPLATE",
    "TRANSFORM",
    "TRUSTED LANGUAGE",
    "TRUSTED PROCEDURAL LANGUAGE",
    "UNLOGGED SEQUENCE",
    "USER MAPPING",
];

/// `ALTER TABLE` actions that set storage or planner details the schema does
/// not model, after the table name.
const TABLE_STORAGE_ACTIONS: &[&str] = &[
    "ALTER COLUMN * SET COMPRESSION",
    "ALTER COLUMN * SET STATISTICS",
    "ALTER COLUMN * SET STORAGE",
    "ALTER COLUMN * SET (",
    "ALTER COLUMN * RESET (",
    "ALTER * SET COMPRESSION",
    "ALTER * SET STATISTICS",
    "ALTER * SET STORAGE",
    "ALTER * SET (",
    "ALTER * RESET (",
    "CLUSTER ON",
    "SET WITHOUT CLUSTER",
    "SET ACCESS METHOD",
    "SET TABLESPACE",
    "SET (",
    "RESET (",
    "FORCE ROW LEVEL SECURITY",
    "NO FORCE ROW LEVEL SECURITY",
    "REPLICA IDENTITY",
];

/// `ALTER INDEX` actions, after the index name. None changes which columns
/// an index covers; renaming is read by the stock grammar.
const INDEX_STORAGE_ACTIONS: &[&str] = &[
    "ATTACH PARTITION",
    "ALTER COLUMN * SET STATISTICS",
    "ALTER * SET STATISTICS",
    "SET TABLESPACE",
    "SET (",
    "RESET (",
    "DEPENDS ON EXTENSION",
    "NO DEPENDS ON EXTENSION",
];

/// Reads the statement at the parser's position, which the stock grammar
/// rejected, and on success advances the parser to its end (before its `;`).
pub(crate) fn rescue(parser: &mut Parser<'_>, dialect: &dyn Dialect) -> Option<Rescued> {
    let tokens = statement_tokens(parser);
    let words: Vec<&TokenWithSpan> = tokens.iter().filter(|token| significant(token)).collect();
    let (first, last) = (words.first()?, words.last()?);
    let span = first.span.union(&last.span);

    let rescued = if let Some(rewritten) = rewrite(&tokens) {
        reparse(dialect, rewritten).map(|statement| Rescued::Statement(Box::new(statement)))
    } else {
        skipped(&words).map(|construct| Rescued::Skipped { construct, span })
    }?;
    for _ in &tokens {
        parser.next_token_no_skip();
    }
    Some(rescued)
}

/// The tokens of the statement at the parser's position, up to its `;`.
///
/// A psql meta-command (`\restrict key`, which `pg_dump` writes since
/// `PostgreSQL` 17.6) ends at the end of its line instead. A `BEGIN ATOMIC`
/// routine body keeps its inner semicolons.
fn statement_tokens(parser: &Parser<'_>) -> Vec<TokenWithSpan> {
    let meta_command = parser.peek_token().token == Token::Backslash;
    let mut tokens = Vec::new();
    let mut started = false;
    let mut depth = 0_usize;
    for n in 0.. {
        let token = parser.peek_nth_token_no_skip(n);
        match &token.token {
            Token::EOF => break,
            Token::SemiColon if depth == 0 && !meta_command => break,
            Token::Whitespace(Whitespace::Newline) if meta_command && started => break,
            Token::Whitespace(Whitespace::SingleLineComment { .. }) if meta_command && started => {
                tokens.push(token);
                break;
            }
            Token::Word(_) if is_word(&token, "BEGIN") && atomic_follows(parser, n) => {
                depth += 1;
            }
            Token::Word(_) if depth > 0 && is_word(&token, "CASE") => depth += 1,
            Token::Word(_) if depth > 0 && is_word(&token, "END") => depth -= 1,
            _ => {}
        }
        started |= significant(&token);
        tokens.push(token);
    }
    tokens
}

fn atomic_follows(parser: &Parser<'_>, mut n: usize) -> bool {
    loop {
        n += 1;
        let token = parser.peek_nth_token_no_skip(n);
        if significant(&token) {
            return is_word(&token, "ATOMIC");
        }
    }
}

const fn significant(token: &TokenWithSpan) -> bool {
    !matches!(token.token, Token::Whitespace(_))
}

fn is_word(token: &TokenWithSpan, expected: &str) -> bool {
    matches!(&token.token, Token::Word(word) if word.quote_style.is_none() && word.value.eq_ignore_ascii_case(expected))
}

/// Whether `words` starts with `phrase`, whose `*` matches any one token and
/// whose `(` matches an opening parenthesis.
fn starts_with(words: &[&TokenWithSpan], phrase: &str) -> bool {
    let parts: Vec<&str> = phrase.split(' ').collect();
    parts.len() <= words.len()
        && parts.iter().zip(words).all(|(part, token)| match *part {
            "*" => true,
            "(" => token.token == Token::LParen,
            word => is_word(token, word),
        })
}

/// The first phrase in `phrases` that `words` starts with.
fn find_phrase<'a>(words: &[&TokenWithSpan], phrases: &[&'a str]) -> Option<&'a str> {
    phrases
        .iter()
        .copied()
        .find(|phrase| starts_with(words, phrase))
}

/// Rewrites the forms the stock grammar rejects for wording the schema does
/// not depend on, or `None` when nothing applies.
fn rewrite(tokens: &[TokenWithSpan]) -> Option<Vec<TokenWithSpan>> {
    let positions: Vec<usize> = (0..tokens.len())
        .filter(|&index| significant(&tokens[index]))
        .collect();
    let word = |n: usize| positions.get(n).map(|&index| &tokens[index]);
    let mut dropped = Vec::new();

    // `CREATE MATERIALIZED VIEW ... WITH [NO] DATA`: whether the view is
    // populated does not change its shape.
    if word(0).is_some_and(|token| is_word(token, "CREATE"))
        && word(1).is_some_and(|token| is_word(token, "MATERIALIZED"))
        && let [.., with, no, data] = positions.as_slice()
        && is_word(&tokens[*data], "DATA")
    {
        if is_word(&tokens[*no], "NO") && is_word(&tokens[*with], "WITH") {
            dropped.extend([*with, *no, *data]);
        } else if is_word(&tokens[*no], "WITH") {
            dropped.extend([*no, *data]);
        }
    }

    // `CREATE INDEX ... ON ONLY parent`: the index is still on `parent`.
    if word(0).is_some_and(|token| is_word(token, "CREATE"))
        && let Some(on) = positions
            .windows(2)
            .position(|pair| is_word(&tokens[pair[0]], "ON") && is_word(&tokens[pair[1]], "ONLY"))
        && (1..on).all(|n| !is_word(&tokens[positions[n]], "TABLE"))
        && (1..on).any(|n| is_word(&tokens[positions[n]], "INDEX"))
    {
        dropped.push(positions[on + 1]);
    }

    // `... AS IDENTITY (SEQUENCE NAME name ...)`: the backing sequence's name
    // is not part of the column.
    for (n, &index) in positions.iter().enumerate() {
        if is_word(&tokens[index], "SEQUENCE")
            && word(n + 1).is_some_and(|token| is_word(token, "NAME"))
            && in_identity_options(tokens, &positions, n)
        {
            dropped.extend([index, positions[n + 1]]);
            // The name: identifiers joined by periods.
            let mut m = n + 2;
            while let Some(token) = word(m) {
                if !matches!(token.token, Token::Word(_)) {
                    break;
                }
                dropped.push(positions[m]);
                if word(m + 1).is_some_and(|token| token.token == Token::Period) {
                    dropped.push(positions[m + 1]);
                    m += 2;
                } else {
                    break;
                }
            }
        }
    }

    (!dropped.is_empty()).then(|| {
        tokens
            .iter()
            .enumerate()
            .filter(|(index, _)| !dropped.contains(index))
            .map(|(_, token)| token.clone())
            .collect()
    })
}

/// Whether significant word `n` sits directly inside `AS IDENTITY (...)`.
fn in_identity_options(tokens: &[TokenWithSpan], positions: &[usize], n: usize) -> bool {
    let mut depth = 0_usize;
    for m in (0..n).rev() {
        match tokens[positions[m]].token {
            Token::RParen => depth += 1,
            Token::LParen if depth > 0 => depth -= 1,
            Token::LParen => {
                return m >= 2
                    && is_word(&tokens[positions[m - 1]], "IDENTITY")
                    && is_word(&tokens[positions[m - 2]], "AS");
            }
            _ => {}
        }
    }
    false
}

/// Reads a rewritten statement with the stock grammar, which must take all
/// of it.
fn reparse(dialect: &dyn Dialect, mut tokens: Vec<TokenWithSpan>) -> Option<Statement> {
    tokens.push(TokenWithSpan::wrap(Token::EOF));
    let mut parser = Parser::new(dialect).with_tokens_with_locations(tokens);
    let statement = parser.parse_statement().ok()?;
    (parser.peek_token().token == Token::EOF).then_some(statement)
}

/// The construct a statement the schema has no use for is reported as, or
/// `None` when the statement is not one of them.
fn skipped(words: &[&TokenWithSpan]) -> Option<String> {
    let head = words.first()?;
    if head.token == Token::Backslash {
        let command = words
            .get(1)
            .map_or_else(String::new, |token| token.token.to_string());
        return Some(format!("psql meta-command \\{command}"));
    }
    if starts_with(words, "SECURITY LABEL") {
        return Some("SECURITY LABEL".to_owned());
    }
    let verb = ["CREATE", "ALTER", "DROP"]
        .into_iter()
        .find(|verb| is_word(head, verb))?;
    let mut rest = &words[1..];
    if verb == "CREATE" && starts_with(rest, "OR REPLACE") {
        rest = &rest[2..];
    }
    if let Some(object) = find_phrase(rest, UNMODELED_OBJECTS)
        && !(verb == "DROP" && words.iter().any(|token| is_word(token, "CASCADE")))
    {
        return Some(format!("{verb} {object}"));
    }
    // The forms below hold one action; a list could hide a modeled change
    // (`SET STATISTICS 100, DROP COLUMN c`) behind an unmodeled one.
    if verb != "ALTER" || has_top_level_comma(words) {
        return None;
    }
    // Ownership is not part of the schema, whatever the object.
    if let ([object, ..], [.., owner, to, _role]) = (rest, words)
        && is_word(owner, "OWNER")
        && is_word(to, "TO")
    {
        return Some(format!(
            "ALTER {} OWNER TO",
            object.token.to_string().to_uppercase()
        ));
    }
    for (object, actions) in [
        ("TABLE", TABLE_STORAGE_ACTIONS),
        ("INDEX", INDEX_STORAGE_ACTIONS),
    ] {
        if starts_with(rest, object)
            && let Some(action) = find_phrase(after_name(&rest[1..]), actions)
        {
            let action = action.replace(" *", "").replace(" (", "");
            return Some(format!("ALTER {object} {action}"));
        }
    }
    None
}

fn has_top_level_comma(words: &[&TokenWithSpan]) -> bool {
    let mut depth = 0_usize;
    words.iter().any(|token| {
        match token.token {
            Token::LParen => depth += 1,
            Token::RParen => depth = depth.saturating_sub(1),
            Token::Comma => return depth == 0,
            _ => {}
        }
        false
    })
}

/// The words after an `[IF EXISTS] [ONLY] name [*]` object reference.
fn after_name<'a, 'b>(mut words: &'a [&'b TokenWithSpan]) -> &'a [&'b TokenWithSpan] {
    if starts_with(words, "IF EXISTS") {
        words = &words[2..];
    }
    if starts_with(words, "ONLY") {
        words = &words[1..];
    }
    // Identifiers joined by periods.
    while let [name, rest @ ..] = words
        && matches!(name.token, Token::Word(_))
    {
        words = rest;
        match words {
            [period, rest @ ..] if period.token == Token::Period => words = rest,
            _ => break,
        }
    }
    match words {
        [star, rest @ ..] if star.token == Token::Mul => rest,
        _ => words,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::dialect::PostgreSqlDialect;

    fn rescue_sql(sql: &str) -> Option<Rescued> {
        let dialect = PostgreSqlDialect {};
        let mut parser = Parser::new(&dialect).try_with_sql(sql).unwrap();
        rescue(&mut parser, &dialect)
    }

    fn skipped_as(sql: &str) -> Option<String> {
        match rescue_sql(sql)? {
            Rescued::Skipped { construct, .. } => Some(construct),
            Rescued::Statement(statement) => panic!("{sql} was read as {statement}"),
        }
    }

    #[test]
    fn unmodeled_objects_are_skipped() {
        for (sql, construct) in [
            (
                "CREATE PROCEDURE public.run(IN a numeric) LANGUAGE sql AS $$ SELECT 1 $$",
                "CREATE PROCEDURE",
            ),
            (
                "ALTER PROCEDURE public.run(IN a numeric) OWNER TO postgres",
                "ALTER PROCEDURE",
            ),
            (
                "CREATE OR REPLACE PROCEDURAL LANGUAGE plsample",
                "CREATE PROCEDURAL LANGUAGE",
            ),
            (
                "CREATE STATISTICS public.s ON id, email FROM public.users",
                "CREATE STATISTICS",
            ),
            (
                "CREATE EVENT TRIGGER et ON ddl_command_start EXECUTE FUNCTION f()",
                "CREATE EVENT TRIGGER",
            ),
            (
                "CREATE CAST (text AS public.email) WITH INOUT AS IMPLICIT",
                "CREATE CAST",
            ),
            (
                "ALTER OPERATOR public.=== (text, text) OWNER TO postgres",
                "ALTER OPERATOR",
            ),
            (
                "ALTER TEXT SEARCH CONFIGURATION c ADD MAPPING FOR word WITH simple",
                "ALTER TEXT SEARCH CONFIGURATION",
            ),
            (
                "SECURITY LABEL FOR selinux ON TABLE public.users IS 'label'",
                "SECURITY LABEL",
            ),
            ("DROP PUBLICATION IF EXISTS pub", "DROP PUBLICATION"),
            (
                "CREATE UNLOGGED SEQUENCE public.s AS integer START WITH 1",
                "CREATE UNLOGGED SEQUENCE",
            ),
            (
                "DROP OPERATOR IF EXISTS public.=== (text, text)",
                "DROP OPERATOR",
            ),
        ] {
            assert_eq!(skipped_as(sql).as_deref(), Some(construct), "{sql}");
        }
    }

    #[test]
    fn modeled_objects_skip_only_details_the_schema_ignores() {
        assert_eq!(
            skipped_as("ALTER TYPE public.mood OWNER TO postgres").as_deref(),
            Some("ALTER TYPE OWNER TO")
        );
        assert_eq!(
            skipped_as("ALTER TABLE ONLY public.users ALTER COLUMN name SET COMPRESSION pglz")
                .as_deref(),
            Some("ALTER TABLE ALTER COLUMN SET COMPRESSION")
        );
        assert_eq!(
            skipped_as("ALTER TABLE public.audit CLUSTER ON audit_pkey").as_deref(),
            Some("ALTER TABLE CLUSTER ON")
        );
        assert_eq!(
            skipped_as("ALTER INDEX public.p ATTACH PARTITION public.c").as_deref(),
            Some("ALTER INDEX ATTACH PARTITION")
        );
        // A list of actions may hold a modeled change, so it stays an error.
        assert!(
            rescue_sql("ALTER TABLE t ALTER COLUMN a SET COMPRESSION pglz, DROP COLUMN b")
                .is_none()
        );
        assert!(rescue_sql("ALTER TABLE t CLUSTER ON i, OWNER TO postgres").is_none());
        // Schemas can embed tables, and drops can cascade to modeled objects.
        assert!(rescue_sql("CREATE SCHEMA s CREATE TABLE t (id int)").is_none());
        assert!(rescue_sql("DROP PUBLICATION IF EXISTS p CASCADE").is_none());
        // A broken statement on a modeled object stays an error.
        assert!(rescue_sql("ALTER TYPE public.mood ADD VALUE").is_none());
        assert!(rescue_sql("ALTER TABLE t ADD COLUMN").is_none());
        assert!(rescue_sql("CREATE TABLE t (id int,)").is_none());
        assert!(rescue_sql("ALTER TABLE t ALTER COLUMN c SET").is_none());
    }

    #[test]
    fn meta_commands_end_at_the_line_end() {
        let dialect = PostgreSqlDialect {};
        let sql = "\\restrict abc123\nSET a = 1;";
        let mut parser = Parser::new(&dialect).try_with_sql(sql).unwrap();
        let Some(Rescued::Skipped { construct, span }) = rescue(&mut parser, &dialect) else {
            panic!("meta-command not skipped");
        };
        assert_eq!(construct, "psql meta-command \\restrict");
        assert_eq!(span.end.line, 1);
        assert!(parser.parse_statement().is_ok());
    }

    #[test]
    fn rewritten_forms_are_read() {
        for sql in [
            "CREATE MATERIALIZED VIEW public.mv AS SELECT 1 AS x WITH NO DATA",
            "CREATE MATERIALIZED VIEW public.mv AS SELECT 1 AS x WITH DATA",
            "CREATE INDEX i ON ONLY public.orders USING btree (user_id)",
            "ALTER TABLE public.a ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (\n    SEQUENCE NAME public.a_id_seq\n    START WITH 1\n    CACHE 1\n)",
        ] {
            assert!(
                matches!(rescue_sql(sql), Some(Rescued::Statement(_))),
                "{sql}"
            );
        }
        // A rewrite that still does not parse is not rescued.
        assert!(rescue_sql("CREATE MATERIALIZED VIEW mv AS WITH NO DATA").is_none());
    }

    #[test]
    fn rescued_statements_consume_their_tokens() {
        let dialect = PostgreSqlDialect {};
        let sql = "CREATE MATERIALIZED VIEW mv AS SELECT 1 AS x WITH NO DATA; SELECT 2";
        let mut parser = Parser::new(&dialect).try_with_sql(sql).unwrap();
        assert!(rescue(&mut parser, &dialect).is_some());
        assert_eq!(parser.peek_token().token, Token::SemiColon);
    }
}
