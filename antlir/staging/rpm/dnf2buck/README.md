dnf2buck
========

Proof-of-concept mechanism to represent an RPM repository with the Buck target
graph.
The rules found in `dnf2buck` are not directly concerned with snapshots, and
assumes that the backing repository is persistent (in other words, that the
backing store holds RPMs for at least as long as they're referenced in the
TARGETS file)

Rules
=====

RPM
---

An `rpm` rule creates an `RpmInfo` provider, which access to:
 - the NEVRA
 - the `.rpm` file blob
 - 3 xml chunks that can be composed into a `repodata/` dir
   - primary, filelists and other

NOTE: while representing the RPM dependency tree is _possible_, it quickly leads
to a massive explosion in dependencies since we don't know how DNF will resolve
dependencies to actually complete a transaction. This could be implemented using
Dynamic Dependencies in buck, where we use dnf to resolve deps, but that is
lower-priority than the overall simplification this representation enables, so
is something left for later .

Repo
----

A `repo` rule is a pure-buck declaration of an RPM repository. It provides a
`repodata/` directory that contains `repomd.xml` and the associated
`{primary,filelists,other}.xml.gz` files.

There is also a `[offline]` sub-target on every `repo` target that will
materialize the `repodata/` directory as well as every RPM file. For large repos
this is ridiculously huge and wasteful, but is extremely useful for offline
testing of small repositories.
