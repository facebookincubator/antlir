---
id: overview
title: Overview
---

## Introduction to RPM

The web has ample documentation on the [RedHat Package Manager ecosystem](https://www.redhat.com/sysadmin/how-manage-packages), so Antlir RPM docs focus on Antlir-specific terminology and behaviors.

To follow these docs, you'll want to be familiar with the following standard terms:

- **Package**: An archive file containing the files to be installed into the OS + package manager metadata, including a list of dependencies.
- **NEVRA**: Name-Epoch-Version-Release-Architecture -- a unique ID for a package within a "universe" (Antlir-specific, defined below). The content of a NEVRA _should_ be immutable (within a universe).

  Importantly, package managers define an old-to-new ordering on EVRAs, and OS management tools typically try to upgrade to the newest package available.

  Sometimes multiple packages share EVRA schemes, and even require matching EVRAs across different package names -- see "package group" below.

- **Repo**: Short for "repository" -- a collection of packages, plus a set of indexes called **repodata** (as XML or Sqlite databases) computed from those packages. Typically hosted at an HTTP URL. The root of the repo is `repomd.xml`, which links to everything else.

  A repo's content may change, as packages are added and removed. As they evolve, repos generally attempt to maintain some kind of backwards compatibility / upgrade path -- e.g. CentOS8.1 should be upgradable to CentOS8.2 and so forth. Not all repos have discrete point releases -- e.g. EPEL7 just moves forward continuously.

  Note that the same package instance (or package name) can be contained in multiple repos, and the package manager will somehow pick one (implementation-defined behavior). Therefore, it is important that repos that are being used together be mutually compatible.

- **Distro release**: A collection of mutually compatible repositories. For example, CentOS7.2 is a distro comprising multiple "standard" repos, while EPEL7 is an add-on repository intended to be compatible with all CentOS7.x distros.
- **yum** / **dnf**: The package manager program, which installs packages and their dependencies into a filesystem root.
- **{yum,dnf}.conf**: Configuration for the package manager, including a list of repos, plus install-time settings, like whether to install optional dependencies, or to validate package GPG signatures.

## Key concepts

### Universe

An RPM-based operating system is typically built from a collection of mutually compatible repos. In Antlir, each repo is assigned a **universe** name, which indicates the intended scope of mutual compatibility of packages within that repo.

An OS may included repos may be larger than a single distro release, e.g. it is possible to have a structure like this:

- CentOS8.3 -- universe "centos8"
- EPEL8 -- universe "centos8"
- CentOS9Stream -- universe "centos9"
- CompanyInternalRepo -- universe "company", statically linked, installed in `/opt/company`.

Key invariants for universes:

- All repos in a universe are mutually compatible.
- Additionally, some universes may also be mutally compatible. In the example above, "centos8" and "centos9" are **not** mutually compatible. However, it is very reasonable for both "centos8" and "centos9" to be compatible with "company".
- For all repos in a universe, a package name must refer to the same piece of software (i.e. it must be upgradable).
- Within a universe, a package NEVRA must uniquely identify the byte contents of the package. Caveat: if package re-signing is commonplace, we may consider supporting an exemption in Antlir for ignoring the signature when the package contents are otherwise identical.

### Package group

Within a universe, a list of related packages, all of which must always have the same installed version (and ought to be installed in one transaction). For example, `systemd`, `systemd-libs`, and `systemd-devel` must be in sync. Each package may be in at most one package group.

### Repo snapshot

Given a collection of repos from mutually compatible universes, Antlir has code (see `antlir/rpm/snapshot_repos.py`) to:

- Given a `{yum,dnf}.conf}, download each repo (atomically within a repo, but not across repos).
- Save the packages and repodata to append-only storage. This step uses a database (`antlir/rpm/repo_db.py`) to avoid redundantly storing objects that were already captured by a previous snapshot.
- Save a build-time index of all the packages in all the repos, called a `RepoSnapshot` (`antlir/rpm/repo_snapshot.py`). This is serialized to SQLite and also uploaded to append-only storage.
- Run `nspawn_in_subvol` containers, which are able to **deterministically** install RPMs from a `RepoSnapshot`. This is normally arranged by committing to source control:
  - The storage ID of the `RepoSnapshot`
  - `{yum,dnf}.conf` corresponding to the snapshot
  - Trusted GPG keys

### Repo snapshot debugging

See commands for [inspecting RPM snapshots in the FAQ](../../faq.md#how-do-i-inspect-the-rpm-snapshot-db).
