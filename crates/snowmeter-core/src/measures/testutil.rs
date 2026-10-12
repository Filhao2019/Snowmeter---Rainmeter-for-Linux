//! Helpers for unit tests: build a `ConfigParser` from INI text in a temp dir.

use crate::env::Environment;
use crate::parser::{ConfigParser, SkinIdentity};

pub struct TestSkin {
    pub _tmp: tempfile::TempDir,
    pub env: Environment,
    pub parser: ConfigParser,
}

pub fn skin(ini: &str) -> TestSkin {
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    let dir = env.skins_dir.join("T");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Main.ini"), ini).unwrap();
    let id = SkinIdentity {
        config: "T".into(),
        root_config: "T".into(),
        file: "Main.ini".into(),
        ini_path: dir.join("Main.ini"),
    };
    let parser = ConfigParser::load(&env, &id).unwrap();
    TestSkin { _tmp: tmp, env, parser }
}

/// Run `f` with a throw-away `MeasureCtx`.
pub fn ctx_run<R>(s: &TestSkin, f: impl FnOnce(&mut super::MeasureCtx) -> R) -> R {
    ctx_with(s, &|_| None, f)
}

pub fn ctx_with<R>(s: &TestSkin, lookup: &dyn Fn(&str) -> Option<f64>, f: impl FnOnce(&mut super::MeasureCtx) -> R) -> R {
    let mut log = Vec::new();
    let dir = s.env.skins_dir.join("T");
    let mut ctx = super::MeasureCtx { env: &s.env, skin_dir: &dir, update_ms: 1000, log: &mut log, lookup, counter: 1 };
    f(&mut ctx)
}
