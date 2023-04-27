/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::fmt::Display;

use configparser::ini::Ini;
use itertools::Itertools;
use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;
use serde_with::DisplayFromStr;
use url::Url;

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct DnfConf {
    install_weak_deps: Option<bool>,
    repos: HashMap<String, RepoConf>,
}

impl DnfConf {
    pub fn builder() -> DnfConfBuilder {
        DnfConfBuilder::default()
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_repo(&mut self, id: String, repo_cfg: RepoConf) {
        self.repos.insert(id, repo_cfg);
    }

    pub fn repos(&self) -> &HashMap<String, RepoConf> {
        &self.repos
    }
}

impl Display for DnfConf {
    #[deny(unused_variables)]
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let Self {
            install_weak_deps,
            repos,
        } = self;
        let mut config = Ini::new();
        if let Some(install_weak_deps) = install_weak_deps {
            config.set(
                "main",
                "install_weak_deps",
                Some(install_weak_deps.to_string()),
            );
        }
        for (id, repo) in repos {
            config.set(
                &id.replace('/', "-"),
                "baseurl",
                Some(
                    repo.base_urls
                        .iter()
                        .map(|u| u.to_string())
                        .join("\n        "),
                ),
            );
            if let Some(name) = &repo.name {
                config.set(id, "name", Some(name.clone()));
            }
        }
        write!(f, "{}", config.writes())
    }
}

#[derive(Debug, Clone, Default)]
pub struct DnfConfBuilder(DnfConf);

impl DnfConfBuilder {
    pub fn install_weak_deps(&mut self, install_weak_deps: bool) -> &mut Self {
        self.0.install_weak_deps = Some(install_weak_deps);
        self
    }

    pub fn add_repo(&mut self, id: String, cfg: impl Into<RepoConf>) -> &mut Self {
        self.0.add_repo(id, cfg.into());
        self
    }

    pub fn build(&self) -> DnfConf {
        self.0.clone()
    }
}
#[serde_as]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RepoConf {
    #[serde_as(as = "Vec<DisplayFromStr>")]
    base_urls: Vec<Url>,
    name: Option<String>,
}

impl RepoConf {
    pub fn base_urls(&self) -> &[Url] {
        &self.base_urls
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl From<Vec<Url>> for RepoConf {
    fn from(urls: Vec<Url>) -> Self {
        Self {
            base_urls: urls,
            name: None,
        }
    }
}

impl From<Url> for RepoConf {
    fn from(u: Url) -> Self {
        vec![u].into()
    }
}

impl From<Vec<http::Uri>> for RepoConf {
    fn from(uris: Vec<http::Uri>) -> Self {
        Self {
            base_urls: uris
                .into_iter()
                .map(|u| u.to_string().parse().expect("definitely valid url"))
                .collect(),
            name: None,
        }
    }
}

impl From<http::Uri> for RepoConf {
    fn from(u: http::Uri) -> Self {
        vec![u].into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder() {
        let dnf_conf = DnfConf::builder()
            .add_repo(
                "foo".into(),
                vec![
                    Url::parse("https://repo.repo/yum/my/repo").expect("valid url"),
                    Url::parse("https://mirror.repo/yum/my/repo").expect("valid Uri"),
                ],
            )
            .build();
        assert_eq!(
            dnf_conf.to_string(),
            r#"[foo]
baseurl=https://repo.repo/yum/my/repo
        https://mirror.repo/yum/my/repo
"#
        );
    }
}
