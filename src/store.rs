use oxigraph::model::{GraphNameRef, NamedNodeRef, QuadRef};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;
use serde_json::{Map, Value};

const REPO_PREFIX: &str = "http://repo.example.org/";

/// Percent-encode characters that are invalid in IRIs.
fn iri_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || "-._~:@!$&'()*+,;=/".contains(c) {
            out.push(c);
        } else {
            // Percent-encode all non-ASCII and special characters
            for b in c.to_string().as_bytes() {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

pub struct CodebaseStore {
    store: Store,
}

impl CodebaseStore {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            store: Store::new()?,
        })
    }

    pub fn query_to_json(&self, sparql: &str) -> Result<Value, Box<dyn std::error::Error>> {
        let results = SparqlEvaluator::new()
            .parse_query(sparql)?
            .on_store(&self.store)
            .execute()?;

        match results {
            QueryResults::Solutions(solutions) => {
                let variables: Vec<String> = solutions
                    .variables()
                    .iter()
                    .map(|v| v.as_str().to_owned())
                    .collect();

                let mut rows = Vec::new();
                for solution in solutions {
                    let solution = solution?;
                    let mut row = Map::new();
                    for var in &variables {
                        let value = solution.get(var.as_str()).map_or(Value::Null, |term| {
                            Value::String(term.to_string())
                        });
                        row.insert(var.clone(), value);
                    }
                    rows.push(Value::Object(row));
                }

                Ok(Value::Array(rows))
            }
            QueryResults::Boolean(b) => Ok(Value::Bool(b)),
            QueryResults::Graph(_) => Err("CONSTRUCT/DESCRIBE queries not supported".into()),
        }
    }

    pub fn insert_triple(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let s = format!("{REPO_PREFIX}{}", iri_escape(subject));
        let p = format!("{REPO_PREFIX}{}", iri_escape(predicate));
        let o = format!("{REPO_PREFIX}{}", iri_escape(object));

        let s_node = NamedNodeRef::new(&s)?;
        let p_node = NamedNodeRef::new(&p)?;
        let o_node = NamedNodeRef::new(&o)?;

        self.store.insert(QuadRef::new(
            s_node,
            p_node,
            o_node,
            GraphNameRef::DefaultGraph,
        ))?;

        Ok(())
    }

    pub fn clear(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.store.clear()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_query() -> Result<(), Box<dyn std::error::Error>> {
        let store = CodebaseStore::new()?;
        store.insert_triple("main.rs", "contains", "fn_main")?;
        store.insert_triple("lib.rs", "contains", "fn_helper")?;

        let json = store.query_to_json(
            "SELECT ?file ?item WHERE { ?file <http://repo.example.org/contains> ?item }",
        )?;

        let rows = json.as_array().unwrap();
        assert_eq!(rows.len(), 2);
        Ok(())
    }

    #[test]
    fn clear_empties_store() -> Result<(), Box<dyn std::error::Error>> {
        let store = CodebaseStore::new()?;
        store.insert_triple("a", "b", "c")?;
        store.clear()?;

        let json =
            store.query_to_json("SELECT ?s ?p ?o WHERE { ?s ?p ?o }")?;
        let rows = json.as_array().unwrap();
        assert!(rows.is_empty());
        Ok(())
    }

    #[test]
    fn query_to_json_returns_boolean_for_ask() -> Result<(), Box<dyn std::error::Error>> {
        let store = CodebaseStore::new()?;
        store.insert_triple("a", "b", "c")?;

        let json = store.query_to_json("ASK { ?s ?p ?o }")?;
        assert_eq!(json, Value::Bool(true));
        Ok(())
    }
}
