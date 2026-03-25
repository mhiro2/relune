//! Runtime diagnostics for the relune CLI.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DoctorStatus {
    Ok,
    Warn,
}

impl std::fmt::Display for DoctorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Warn => write!(f, "warn"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorCheck {
    name: &'static str,
    status: DoctorStatus,
    detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorReport {
    overall: DoctorStatus,
    checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    fn collect() -> Self {
        let checks = vec![
            DoctorCheck {
                name: "version",
                status: DoctorStatus::Ok,
                detail: format!("relune {}", env!("CARGO_PKG_VERSION")),
            },
            doctor_input_check(),
            doctor_introspect_check(),
        ];

        let overall = checks
            .iter()
            .map(|check| check.status)
            .max()
            .unwrap_or(DoctorStatus::Ok);

        Self { overall, checks }
    }
}

fn doctor_input_check() -> DoctorCheck {
    DoctorCheck {
        name: "inputs",
        status: DoctorStatus::Ok,
        detail: "sql, sql-text, and schema-json inputs available".to_string(),
    }
}

fn doctor_introspect_check() -> DoctorCheck {
    if relune_app::introspection_enabled() {
        let backends = relune_app::supported_introspection_backends().join(", ");
        DoctorCheck {
            name: "introspect",
            status: DoctorStatus::Ok,
            detail: format!("db-url support compiled with {backends} backends"),
        }
    } else {
        DoctorCheck {
            name: "introspect",
            status: DoctorStatus::Warn,
            detail: "db-url input is unavailable in this build".to_string(),
        }
    }
}

/// Run the doctor command.
pub(crate) fn run_doctor() {
    let report = DoctorReport::collect();

    println!("relune doctor: {}", report.overall);
    for check in report.checks {
        println!("- {}: {} ({})", check.name, check.status, check.detail);
    }
    println!("- note: doctor reports runtime-visible capabilities, not database connectivity");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_reports_runtime_visible_capabilities() {
        let report = DoctorReport::collect();

        assert!(
            report
                .checks
                .iter()
                .any(|check| check.name == "inputs" && check.status == DoctorStatus::Ok)
        );
        assert!(report.checks.iter().any(|check| check.name == "introspect"));
    }
}
