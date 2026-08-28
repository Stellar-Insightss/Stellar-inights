#[cfg(test)]
mod tests {
    #[test]
    fn test_privilege_escalation_docs_and_artifacts() {
        assert!(std::path::Path::new("../docs/contract-privilege-graph.md").exists() || std::path::Path::new("docs/contract-privilege-graph.md").exists());
        assert!(std::path::Path::new("upgrade/src/scope.rs").exists() || std::path::Path::new("contracts/upgrade/src/scope.rs").exists());
        assert!(std::path::Path::new("upgrade/tests/privilege_escalation_test.rs").exists() || std::path::Path::new("contracts/upgrade/tests/privilege_escalation_test.rs").exists());
    }
}
