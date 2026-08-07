//! The `snippets` table. The module owns it and creates it idempotently at
//! boot, the same `CREATE TABLE IF NOT EXISTS` pattern every other module uses.

use sqlx::{Row, SqlitePool};

use crate::{Snippet, SnippetDraft, SnippetError, SnippetResult};

/// What an unlabelled snippet gets. Nothing parses the language — there is no
/// highlighting — so this is a label for the human, not a mode switch.
pub const DEFAULT_LANGUAGE: &str = "plaintext";

/// Tags are stored as one delimited string rather than JSON (which is what
/// `api_requests` does with headers). That is safe *because* it is enforced:
/// [`normalize_tags`] splits on this character, so no tag can ever contain it
/// and the join/split round trip is lossless. Header *values* are free-form,
/// which is why that table needs JSON and this one does not.
const TAG_SEPARATOR: char = ',';

/// The escape character for `LIKE`. Backslash rather than something exotic
/// because it is what a reader expects; it is itself escaped by
/// [`escape_like`], so a snippet search for `\` still works.
const LIKE_ESCAPE: char = '\\';

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Trimmed, lowercased, deduped, order preserved — and split on
/// [`TAG_SEPARATOR`], so a client that sends `["react,hooks"]` as one string
/// and one that sends `["react", "hooks"]` store the same thing.
///
/// Lowercasing is what makes tags behave like tags: `React` and `react` are
/// one label, not two rows in a filter list.
pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in tags {
        for part in raw.split(TAG_SEPARATOR) {
            let tag = part.trim().to_lowercase();
            if tag.is_empty() || out.contains(&tag) {
                continue;
            }
            out.push(tag);
        }
    }
    out
}

/// Blank becomes [`DEFAULT_LANGUAGE`]; anything else is trimmed and lowercased
/// so `Rust`, `rust ` and `rust` group together in the UI.
pub fn normalize_language(language: &str) -> String {
    let language = language.trim().to_lowercase();
    if language.is_empty() {
        DEFAULT_LANGUAGE.to_string()
    } else {
        language
    }
}

/// Neutralise `LIKE`'s own wildcards in user input.
///
/// Without this, typing `%` into the search box matches every snippet and `_`
/// matches any single character — the search would quietly answer a different
/// question than the one asked. Not a security boundary (the value is bound,
/// never formatted into SQL), just correctness.
fn escape_like(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for ch in query.chars() {
        if ch == '%' || ch == '_' || ch == LIKE_ESCAPE {
            out.push(LIKE_ESCAPE);
        }
        out.push(ch);
    }
    out
}

pub async fn init(pool: &SqlitePool) -> SnippetResult<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS snippets (
            id         TEXT PRIMARY KEY,
            title      TEXT NOT NULL,
            language   TEXT NOT NULL,
            body       TEXT NOT NULL,
            tags       TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert or update, decided by `draft.id`.
///
/// An id that names no row is an error rather than an insert-with-that-id:
/// the only way to hold one is to have loaded a snippet that has since been
/// deleted, and silently resurrecting it under the old id would hide that.
pub async fn save_snippet(pool: &SqlitePool, draft: &SnippetDraft) -> SnippetResult<Snippet> {
    let title = draft.title.trim();
    if title.is_empty() {
        return Err(SnippetError::Invalid("snippet title is empty".into()));
    }
    // The body is stored byte-for-byte. Trimming it would be wrong for the
    // thing this feature exists to hold: indentation and a trailing newline
    // are part of a code fragment, not noise around it.
    let tags = normalize_tags(&draft.tags);
    let now = now_ms();

    let Some(id) = draft.id.as_deref() else {
        let snippet = Snippet {
            id: new_id(),
            title: title.to_string(),
            language: normalize_language(&draft.language),
            body: draft.body.clone(),
            tags,
            created_at: now,
            updated_at: now,
        };
        sqlx::query(
            "INSERT INTO snippets (id, title, language, body, tags, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(&snippet.id)
        .bind(&snippet.title)
        .bind(&snippet.language)
        .bind(&snippet.body)
        .bind(join_tags(&snippet.tags))
        .bind(snippet.created_at)
        .bind(snippet.updated_at)
        .execute(pool)
        .await?;
        return Ok(snippet);
    };

    let result = sqlx::query(
        "UPDATE snippets SET title = ?1, language = ?2, body = ?3, tags = ?4, updated_at = ?5
         WHERE id = ?6",
    )
    .bind(title)
    .bind(normalize_language(&draft.language))
    .bind(&draft.body)
    .bind(join_tags(&tags))
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(SnippetError::NotFound(format!("snippet not found: {id}")));
    }
    get_snippet(pool, id).await
}

pub async fn list_snippets(pool: &SqlitePool) -> SnippetResult<Vec<Snippet>> {
    let rows = sqlx::query(
        "SELECT id, title, language, body, tags, created_at, updated_at
         FROM snippets ORDER BY updated_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(map_snippet).collect())
}

/// Substring search across title, language, tags and body.
///
/// **Why `LIKE` and not FTS5**, given the index module already runs FTS5 next
/// door and it would have been the flashier answer:
///
///   * **FTS5 is word-indexed, and this content is code.** `unicode61` breaks
///     on non-alphanumerics, so `useQuery` is one token and a search for
///     `Query` — the way anyone actually looks for it — finds nothing. Same
///     for `->`, `::`, `--no-verify`, a fragment of a URL. Substring matching
///     is not a cheaper approximation of what a snippet library wants; it is
///     the correct semantics, and full-text search is the approximation.
///   * **The volume does not argue for an index.** Snippets are hand-written
///     by one person. A few hundred rows of a few KB each is well under a
///     millisecond to scan, and the query runs on a keystroke against a local
///     file — there is no network in the loop to hide behind either way.
///   * **An index is a second source of truth.** FTS5 means a shadow table
///     that every write has to keep in step, plus sanitising the query so
///     `"`, `*`, `-` and `NEAR` are not read as operators. `devos-index`
///     carries both of those costs because it indexes whole repositories;
///     paying them here would buy a slower answer to a worse question.
///
/// This is a single substring, not an AND of terms: `docker compose` finds
/// snippets containing that exact phrase, not ones mentioning both words
/// apart. Revisit if the library ever gets big enough that phrase search
/// stops being enough.
///
/// Case-insensitivity comes from SQLite's default `LIKE`, which folds ASCII
/// only — a search for `STRASSE` will not find `straße`. Acceptable for
/// keywords and code; worth knowing before someone files it as a bug.
pub async fn search_snippets(pool: &SqlitePool, query: &str) -> SnippetResult<Vec<Snippet>> {
    let query = query.trim();
    if query.is_empty() {
        return list_snippets(pool).await;
    }
    let pattern = format!("%{}%", escape_like(query));
    let rows = sqlx::query(
        "SELECT id, title, language, body, tags, created_at, updated_at
         FROM snippets
         WHERE title    LIKE ?1 ESCAPE '\\'
            OR language LIKE ?1 ESCAPE '\\'
            OR tags     LIKE ?1 ESCAPE '\\'
            OR body     LIKE ?1 ESCAPE '\\'
         ORDER BY updated_at DESC, id DESC",
    )
    .bind(&pattern)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(map_snippet).collect())
}

pub async fn delete_snippet(pool: &SqlitePool, id: &str) -> SnippetResult<()> {
    let result = sqlx::query("DELETE FROM snippets WHERE id = ?1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(SnippetError::NotFound(format!("snippet not found: {id}")));
    }
    Ok(())
}

async fn get_snippet(pool: &SqlitePool, id: &str) -> SnippetResult<Snippet> {
    let row = sqlx::query(
        "SELECT id, title, language, body, tags, created_at, updated_at
         FROM snippets WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| SnippetError::NotFound(format!("snippet not found: {id}")))?;
    Ok(map_snippet(&row))
}

fn join_tags(tags: &[String]) -> String {
    tags.join(&TAG_SEPARATOR.to_string())
}

fn split_tags(stored: &str) -> Vec<String> {
    stored
        .split(TAG_SEPARATOR)
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect()
}

fn map_snippet(row: &sqlx::sqlite::SqliteRow) -> Snippet {
    Snippet {
        id: row.get("id"),
        title: row.get("title"),
        language: row.get("language"),
        body: row.get("body"),
        tags: split_tags(row.get("tags")),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn test_pool() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("snippets.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .unwrap();
        init(&pool).await.unwrap();
        (dir, pool)
    }

    fn draft(title: &str, body: &str) -> SnippetDraft {
        SnippetDraft {
            id: None,
            title: title.into(),
            language: "rust".into(),
            body: body.into(),
            tags: vec![],
        }
    }

    async fn seed(pool: &SqlitePool, title: &str, body: &str) -> Snippet {
        save_snippet(pool, &draft(title, body)).await.unwrap()
    }

    #[tokio::test]
    async fn save_list_delete_roundtrip() {
        let (_dir, pool) = test_pool().await;

        let saved = save_snippet(
            &pool,
            &SnippetDraft {
                id: None,
                title: "  Reset a branch  ".into(),
                language: "  Shell  ".into(),
                body: "  git reset --hard origin/main\n".into(),
                tags: vec!["Git".into(), "  danger ".into(), "git".into(), "".into()],
            },
        )
        .await
        .unwrap();

        assert_eq!(saved.title, "Reset a branch", "title trimmed");
        assert_eq!(saved.language, "shell", "language trimmed and lowercased");
        assert_eq!(
            saved.body, "  git reset --hard origin/main\n",
            "body stored byte-for-byte: indentation is content here"
        );
        assert_eq!(
            saved.tags,
            vec!["git", "danger"],
            "tags lowercased, deduped, blanks dropped, order kept"
        );
        assert_eq!(saved.created_at, saved.updated_at, "new snippet, one clock");

        let listed = list_snippets(&pool).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0], saved, "what came back is what went in");

        delete_snippet(&pool, &saved.id).await.unwrap();
        assert!(list_snippets(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_id_updates_in_place_and_no_id_inserts() {
        let (_dir, pool) = test_pool().await;
        let first = seed(&pool, "Original", "one").await;

        let edited = save_snippet(
            &pool,
            &SnippetDraft {
                id: Some(first.id.clone()),
                title: "Edited".into(),
                language: "python".into(),
                body: "two".into(),
                tags: vec!["updated".into()],
            },
        )
        .await
        .unwrap();

        assert_eq!(edited.id, first.id, "same row");
        assert_eq!(edited.title, "Edited");
        assert_eq!(edited.body, "two");
        assert_eq!(edited.tags, vec!["updated"]);
        assert_eq!(
            edited.created_at, first.created_at,
            "an edit must not rewrite when the snippet was created"
        );
        assert!(
            edited.updated_at >= first.updated_at,
            "an edit moves updated_at forward"
        );
        assert_eq!(
            list_snippets(&pool).await.unwrap().len(),
            1,
            "an update is not a second row"
        );

        // The same fields with no id are a different snippet, not the same one
        // again — this is the whole insert-vs-update contract.
        save_snippet(&pool, &draft("Edited", "two")).await.unwrap();
        assert_eq!(list_snippets(&pool).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn rejects_a_blank_title() {
        let (_dir, pool) = test_pool().await;
        for title in ["", "   ", "\t\n"] {
            assert!(
                matches!(
                    save_snippet(&pool, &draft(title, "body")).await,
                    Err(SnippetError::Invalid(_))
                ),
                "blank title {title:?} was accepted"
            );
        }
        assert!(
            list_snippets(&pool).await.unwrap().is_empty(),
            "a rejected save must not leave a row behind"
        );

        // An existing snippet cannot be blanked out by an edit either.
        let saved = seed(&pool, "Keeps its name", "body").await;
        assert!(matches!(
            save_snippet(
                &pool,
                &SnippetDraft {
                    id: Some(saved.id.clone()),
                    title: "  ".into(),
                    language: "rust".into(),
                    body: "body".into(),
                    tags: vec![],
                },
            )
            .await,
            Err(SnippetError::Invalid(_))
        ));
        assert_eq!(
            list_snippets(&pool).await.unwrap()[0].title,
            "Keeps its name"
        );
    }

    #[tokio::test]
    async fn deleting_or_editing_an_id_that_is_gone_is_an_error() {
        let (_dir, pool) = test_pool().await;
        let saved = seed(&pool, "Doomed", "body").await;

        delete_snippet(&pool, &saved.id).await.unwrap();
        assert!(
            matches!(
                delete_snippet(&pool, &saved.id).await,
                Err(SnippetError::NotFound(_))
            ),
            "deleting the same snippet twice must not report success"
        );
        assert!(matches!(
            delete_snippet(&pool, "never-existed").await,
            Err(SnippetError::NotFound(_))
        ));

        // Saving through a stale id resurrects nothing.
        assert!(matches!(
            save_snippet(
                &pool,
                &SnippetDraft {
                    id: Some(saved.id.clone()),
                    title: "Back from the dead".into(),
                    language: "rust".into(),
                    body: "body".into(),
                    tags: vec![],
                },
            )
            .await,
            Err(SnippetError::NotFound(_))
        ));
        assert!(list_snippets(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn search_matches_every_field_and_only_the_right_rows() {
        let (_dir, pool) = test_pool().await;

        save_snippet(
            &pool,
            &SnippetDraft {
                id: None,
                title: "Rebase onto main".into(),
                language: "shell".into(),
                body: "git rebase origin/main".into(),
                tags: vec!["git".into()],
            },
        )
        .await
        .unwrap();
        save_snippet(
            &pool,
            &SnippetDraft {
                id: None,
                title: "Debounced input".into(),
                language: "typescript".into(),
                body: "const timer = setTimeout(fn, 300);".into(),
                tags: vec!["react".into(), "hooks".into()],
            },
        )
        .await
        .unwrap();

        let titles =
            |found: Vec<Snippet>| -> Vec<String> { found.into_iter().map(|s| s.title).collect() };

        // Title only: "Rebase" appears in one title and in no other row's
        // anything.
        assert_eq!(
            titles(search_snippets(&pool, "Rebase").await.unwrap()),
            vec!["Rebase onto main"]
        );
        // Body only: "setTimeout" is nowhere in a title, a tag or a language.
        assert_eq!(
            titles(search_snippets(&pool, "setTimeout").await.unwrap()),
            vec!["Debounced input"],
            "a term that lives only in the body still finds its snippet"
        );
        // Tag only, and language only.
        assert_eq!(
            titles(search_snippets(&pool, "hooks").await.unwrap()),
            vec!["Debounced input"]
        );
        assert_eq!(
            titles(search_snippets(&pool, "shell").await.unwrap()),
            vec!["Rebase onto main"]
        );

        // Case-insensitive both ways: the stored text is lowercase and the
        // query is not, and vice versa.
        assert_eq!(
            titles(search_snippets(&pool, "SETTIMEOUT").await.unwrap()),
            vec!["Debounced input"]
        );
        assert_eq!(
            titles(search_snippets(&pool, "rebase onto MAIN").await.unwrap()),
            vec!["Rebase onto main"]
        );

        // Substring, not word — the reason this is `LIKE` and not FTS5.
        assert_eq!(
            titles(search_snippets(&pool, "Timeout").await.unwrap()),
            vec!["Debounced input"],
            "a fragment inside an identifier matches; FTS5 would not"
        );

        // And the negative case, which is the half that proves the rest.
        assert!(
            search_snippets(&pool, "kubernetes")
                .await
                .unwrap()
                .is_empty(),
            "a term in no snippet matches nothing"
        );
        assert!(
            search_snippets(&pool, "rebase setTimeout")
                .await
                .unwrap()
                .is_empty(),
            "search is one substring, not an AND of terms — say so honestly"
        );
    }

    #[tokio::test]
    async fn search_wildcards_are_literal_and_a_blank_query_lists_everything() {
        let (_dir, pool) = test_pool().await;
        seed(&pool, "Format specifier", "printf '%s\\n' \"$x\"").await;
        seed(&pool, "Plain", "nothing special here").await;

        // Without escaping, `%` is LIKE's "match anything" and this returns
        // both rows — the search would answer a question nobody asked.
        assert_eq!(
            search_snippets(&pool, "%").await.unwrap().len(),
            1,
            "`%` is a character to look for, not a wildcard"
        );
        assert_eq!(search_snippets(&pool, "%s").await.unwrap().len(), 1);
        assert_eq!(
            search_snippets(&pool, "_").await.unwrap().len(),
            0,
            "`_` matches an underscore, not any single character"
        );

        for blank in ["", "   ", "\t"] {
            assert_eq!(
                search_snippets(&pool, blank).await.unwrap().len(),
                2,
                "a blank query {blank:?} is not a filter"
            );
        }
    }

    #[tokio::test]
    async fn lists_and_searches_most_recently_edited_first() {
        let (_dir, pool) = test_pool().await;
        let first = seed(&pool, "Saved first", "shared").await;
        let second = seed(&pool, "Saved second", "shared").await;

        // The two saves above can land in the same millisecond, which would
        // leave the order decided by a random UUID tie-break. Stamp the times
        // explicitly so this asserts the ordering rule rather than the clock.
        for (id, updated_at) in [(&first.id, 2_000i64), (&second.id, 1_000i64)] {
            sqlx::query("UPDATE snippets SET updated_at = ?1 WHERE id = ?2")
                .bind(updated_at)
                .bind(id)
                .execute(&pool)
                .await
                .unwrap();
        }

        let ids: Vec<String> = list_snippets(&pool)
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(
            ids,
            vec![first.id.clone(), second.id.clone()],
            "newest edit first, whatever order the rows were created in"
        );

        let found: Vec<String> = search_snippets(&pool, "shared")
            .await
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(found, ids, "search uses the same order as the list");
    }

    #[test]
    fn tag_normalization_keeps_the_stored_format_lossless() {
        // Whatever shape a tag arrives in, it cannot contain the separator —
        // which is what makes join/split a safe storage format.
        let tags = normalize_tags(&[
            "React, Hooks".into(),
            " react ".into(),
            ",,".into(),
            "State Management".into(),
        ]);
        assert_eq!(tags, vec!["react", "hooks", "state management"]);
        assert_eq!(split_tags(&join_tags(&tags)), tags);
        assert!(normalize_tags(&[]).is_empty());
        assert!(normalize_tags(&["   ".into(), ",".into()]).is_empty());
    }

    #[test]
    fn language_defaults_rather_than_going_blank() {
        assert_eq!(normalize_language("  TypeScript "), "typescript");
        for blank in ["", "   ", "\t"] {
            assert_eq!(normalize_language(blank), DEFAULT_LANGUAGE);
        }
    }
}
