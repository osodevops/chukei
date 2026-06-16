//! Hard and soft fingerprints (PRD §9.1).
//!
//! - **hard** — blake3 of the canonical SQL *plus its literal bindings*;
//!   exact result identity, safe as a cache key. Two queries that differ
//!   only in whitespace/case/comments share it; two queries with different
//!   literal values never do — they return different data.
//! - **soft** — hash over the structural feature set; clusters queries that
//!   are shaped alike regardless of literals or projection order.

use super::features::QueryFeatures;

pub fn hard(canonical_sql: &str, literals: &[String]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(canonical_sql.as_bytes());
    for literal in literals {
        // Length-prefix each literal so ["ab","c"] != ["a","bc"].
        hasher.update(&(literal.len() as u64).to_le_bytes());
        hasher.update(literal.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

pub fn soft(features: &QueryFeatures) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    for table in &features.tables {
        hasher.update(table.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(&[features.joins_count.min(255) as u8]);
    for agg in &features.aggregates {
        hasher.update(agg.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(&[
        features.has_group_by as u8,
        features.has_order_by as u8,
        features.is_select_star as u8,
        features.deterministic as u8,
        // Bucket predicate count so trivial filter additions stay in-cluster.
        bucket(features.predicates_count),
    ]);
    let mut out = [0u8; 16];
    out.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
    out
}

fn bucket(n: usize) -> u8 {
    match n {
        0 => 0,
        1..=2 => 1,
        3..=5 => 2,
        _ => 3,
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sql::{features::extract, parse::parse_single};

    #[test]
    fn soft_fingerprint_clusters_similar_queries() {
        let a = soft(&extract(
            &parse_single("SELECT a, b FROM t WHERE x = 1").unwrap(),
        ));
        let b = soft(&extract(
            &parse_single("SELECT b, a FROM t WHERE x = 2 AND y = 3").unwrap(),
        ));
        assert_eq!(a, b, "same table/shape should cluster");
    }

    #[test]
    fn hard_fingerprint_distinguishes_literals() {
        let canon = "select a from t where x = ?";
        let a = hard(canon, &["1".into()]);
        let b = hard(canon, &["2".into()]);
        assert_ne!(
            a, b,
            "different literals → different results → different keys"
        );
        assert_eq!(a, hard(canon, &["1".into()]));
        // Length-prefixing: ["ab","c"] must not collide with ["a","bc"].
        assert_ne!(
            hard(canon, &["ab".into(), "c".into()]),
            hard(canon, &["a".into(), "bc".into()])
        );
    }

    #[test]
    fn hex_renders() {
        assert_eq!(hex(&[0xab, 0x01]), "ab01");
    }
}
