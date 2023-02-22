/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::Path;

use buck_label::Label;
use features::types::BuckOutSource;
use features::types::Layer;
use features::types::PathInLayer;
use features::usergroup::GroupName;
use features::usergroup::UserName;
use shape::ShapePath;
use target_tagger::target_tagged_image_source_t;

pub trait FromNew<N> {
    fn from_new(new: N) -> Self;
}

pub trait IntoShape<S> {
    fn into_shape(self) -> S;
}

impl<T, S> IntoShape<S> for T
where
    S: FromNew<T>,
{
    fn into_shape(self) -> S {
        S::from_new(self)
    }
}

impl FromNew<&Path> for ShapePath {
    fn from_new(new: &Path) -> Self {
        Self::new(new.to_str().expect("all our paths are utf8").to_owned())
    }
}

impl FromNew<PathInLayer<'_>> for ShapePath {
    fn from_new(new: PathInLayer) -> Self {
        new.path().into_shape()
    }
}

impl FromNew<PathInLayer<'_>> for String {
    fn from_new(new: PathInLayer) -> Self {
        new.path()
            .to_str()
            .expect("all our paths are utf8")
            .to_owned()
    }
}

impl FromNew<BuckOutSource<'_>> for ShapePath {
    fn from_new(new: BuckOutSource) -> Self {
        new.path().into_shape()
    }
}

impl FromNew<Layer<'_>> for target_tagged_image_source_t {
    fn from_new(new: Layer) -> Self {
        Self {
            layer: Some(new.into_shape()),
            source: None,
            path: None,
        }
    }
}

impl FromNew<Layer<'_>> for BTreeMap<String, String> {
    fn from_new(new: Layer) -> Self {
        BTreeMap::from([("__BUCK_LAYER_TARGET".to_string(), new.label().to_string())])
    }
}

impl FromNew<Label<'_>> for BTreeMap<String, String> {
    fn from_new(new: Label) -> Self {
        BTreeMap::from([("__BUCK_TARGET".to_string(), new.to_string())])
    }
}

impl FromNew<(Layer<'_>, PathInLayer<'_>)> for target_tagged_image_source_t {
    fn from_new(new: (Layer, PathInLayer)) -> Self {
        let (layer_label, path) = new;
        Self {
            layer: Some(layer_label.into_shape()),
            source: None,
            path: Some(path.into_shape()),
        }
    }
}

impl FromNew<BuckOutSource<'_>> for target_tagged_image_source_t {
    fn from_new(new: BuckOutSource) -> Self {
        Self {
            layer: None,
            source: None,
            path: Some(
                new.path()
                    .to_str()
                    .expect("all our paths are utf8")
                    .to_owned(),
            ),
        }
    }
}

impl FromNew<UserName<'_>> for String {
    fn from_new(new: UserName) -> Self {
        new.name().to_owned()
    }
}

impl FromNew<GroupName<'_>> for String {
    fn from_new(new: GroupName) -> Self {
        new.name().to_owned()
    }
}

// New feature formats to shapes below here

impl FromNew<features::Feature<'_>> for serde_json::Value {
    fn from_new(new: features::Feature) -> Self {
        let mut v = serde_json::json!({
            "target": new.label,
        });
        let obj = v.as_object_mut().expect("this is an object");
        let key = match &new.data {
            features::Data::Clone(_) => "clone",
            features::Data::EnsureDirsExist(_) => "ensure_subdirs_exist",
            features::Data::Genrule(_) => "genrule_layer",
            features::Data::Install(_) => "install_files",
            features::Data::Meta(features::meta_kv::Meta::Store(_)) => "meta_key_value_store",
            features::Data::Meta(features::meta_kv::Meta::Remove(_)) => {
                "remove_meta_key_value_store"
            }
            features::Data::Mount(_) => "mounts",
            features::Data::ParentLayer(_) => "parent_layer",
            features::Data::ReceiveSendstream(_) => "layer_from_package",
            features::Data::Remove(_) => "remove_paths",
            features::Data::Requires(_) => "requires",
            features::Data::Rpm(_) => "rpms",
            features::Data::Rpm2(_) => {
                unimplemented!("this feature does not exist in the target graph")
            }
            features::Data::EnsureFileSymlink(_) => "symlinks_to_files",
            features::Data::EnsureDirSymlink(_) => "symlinks_to_dirs",
            features::Data::Tarball(_) => "tarballs",
            features::Data::User(_) => "users",
            features::Data::UserMod(_) => "usermod",
            features::Data::Group(_) => "groups",
        };
        let data: serde_json::Value = match new.data {
            features::Data::Clone(x) => serde_json::to_value(clone::clone_t::from_new(x)),
            features::Data::EnsureDirsExist(x) => {
                serde_json::to_value(ensure_subdirs_exist::ensure_subdirs_exist_t::from_new(x))
            }
            features::Data::Genrule(x) => {
                serde_json::to_value(genrule_layer::genrule_layer_t::from_new(x))
            }
            features::Data::Install(x) => {
                serde_json::to_value(install::install_files_t::from_new(x))
            }
            features::Data::Meta(features::meta_kv::Meta::Store(x)) => serde_json::to_value(
                meta_key_value_store::meta_key_value_store_item_t::from_new(x),
            ),
            features::Data::Meta(features::meta_kv::Meta::Remove(x)) => serde_json::to_value(
                meta_key_value_store::remove_meta_key_value_store_item_t::from_new(x),
            ),
            features::Data::Mount(features::mount::Mount::Host(x)) => {
                serde_json::to_value(serde_json::Value::from_new(x))
            }
            features::Data::Mount(features::mount::Mount::Layer(x)) => {
                serde_json::to_value(serde_json::Value::from_new(x))
            }
            features::Data::ParentLayer(x) => serde_json::to_value(serde_json::Value::from_new(x)),
            features::Data::ReceiveSendstream(x) => {
                serde_json::to_value(from_package::layer_from_package_t::from_new(x))
            }
            features::Data::Remove(x) => serde_json::to_value(remove::remove_paths_t::from_new(x)),
            features::Data::Requires(x) => serde_json::to_value(requires::requires_t::from_new(x)),
            features::Data::Rpm(x) => serde_json::to_value(rpms::rpm_action_item_t::from_new(x)),
            features::Data::Rpm2(_) => {
                unimplemented!("this feature does not exist in the target graph")
            }
            features::Data::EnsureFileSymlink(x) => {
                serde_json::to_value(symlink::symlink_t::from_new(x))
            }
            features::Data::EnsureDirSymlink(x) => {
                serde_json::to_value(symlink::symlink_t::from_new(x))
            }
            features::Data::Tarball(x) => serde_json::to_value(tarball::tarball_t::from_new(x)),
            features::Data::User(x) => serde_json::to_value(usergroup::user_t::from_new(x)),
            features::Data::UserMod(x) => serde_json::to_value(usergroup::usermod_t::from_new(x)),
            features::Data::Group(x) => serde_json::to_value(usergroup::group_t::from_new(x)),
        }
        .expect("json conversion will not fail");
        obj.insert(key.to_owned(), vec![data].into());
        v
    }
}

impl FromNew<features::clone::Clone<'_>> for clone::clone_t {
    fn from_new(new: features::clone::Clone) -> Self {
        Self {
            dest: new.dst_path.into_shape(),
            omit_outer_dir: new.omit_outer_dir,
            pre_existing_dest: new.pre_existing_dest,
            source: (new.src_layer.clone(), new.src_path).into_shape(),
            source_layer: new.src_layer.into_shape(),
        }
    }
}

impl FromNew<features::ensure_dirs_exist::EnsureDirsExist<'_>>
    for ensure_subdirs_exist::ensure_subdirs_exist_t
{
    fn from_new(new: features::ensure_dirs_exist::EnsureDirsExist) -> Self {
        Self {
            into_dir: new.into_dir.into_shape(),
            subdirs_to_create: new.subdirs_to_create.into_shape(),
            mode: new.mode.0.into(),
            user: new.user.into_shape(),
            group: new.group.into_shape(),
        }
    }
}

impl FromNew<features::genrule::Genrule<'_>> for genrule_layer::genrule_layer_t {
    fn from_new(new: features::genrule::Genrule) -> Self {
        Self {
            cmd: new.cmd.into_iter().map(Cow::into_owned).collect(),
            user: new.user.into_shape(),
            container_opts: new.container_opts,
            bind_repo_ro: new.bind_repo_ro,
            boot: new.boot,
        }
    }
}

impl FromNew<features::install::Install<'_>> for install::install_files_t {
    fn from_new(new: features::install::Install) -> Self {
        Self {
            dest: new.dst.into_shape(),
            source: new.src.into_shape(),
            mode: new.mode.map(|m| m.0.into()),
            user: new.user.into_shape(),
            group: new.group.into_shape(),
            separate_debug_symbols: new.separate_debug_symbols,
        }
    }
}

impl FromNew<features::meta_kv::Store<'_>> for meta_key_value_store::meta_key_value_store_item_t {
    fn from_new(new: features::meta_kv::Store) -> Self {
        Self {
            key: new.key.into_owned(),
            value: new.value.into_owned(),
            require_keys: new.require_keys.into_iter().map(Cow::into_owned).collect(),
            store_if_not_exists: new.store_if_not_exists,
        }
    }
}

impl FromNew<features::meta_kv::Remove<'_>>
    for meta_key_value_store::remove_meta_key_value_store_item_t
{
    fn from_new(new: features::meta_kv::Remove) -> Self {
        Self {
            key: new.key.into_owned(),
        }
    }
}

impl FromNew<features::mount::HostMount<'_>> for serde_json::Value {
    fn from_new(new: features::mount::HostMount) -> Self {
        serde_json::json!({
            "mount_config": {
                "build_source": {
                    "source": new.src,
                    "type": "host",
                },
                "default_mountpoint": new.mountpoint,
                "is_directory": new.is_directory,
            }
        })
    }
}

impl FromNew<features::mount::LayerMount<'_>> for serde_json::Value {
    fn from_new(new: features::mount::LayerMount) -> Self {
        serde_json::json!({
            "mountpoint": new.mountpoint,
            "target": <BTreeMap::<String, String>>::from_new(new.src.label().clone()),
            "mount_config": null,
        })
    }
}

impl FromNew<features::parent_layer::ParentLayer<'_>> for serde_json::Value {
    fn from_new(new: features::parent_layer::ParentLayer) -> Self {
        serde_json::json!({
            "subvol": <BTreeMap::<String, String>>::from_new(new.layer),
        })
    }
}

impl FromNew<features::receive_sendstream::ReceiveSendstream<'_>>
    for from_package::layer_from_package_t
{
    fn from_new(new: features::receive_sendstream::ReceiveSendstream) -> Self {
        Self {
            source: new.src.into_shape(),
            format: match new.format {
                features::receive_sendstream::Format::Sendstream => "sendstream",
                features::receive_sendstream::Format::SendstreamV2 => "sendstream.v2",
            }
            .to_string(),
        }
    }
}

impl FromNew<features::remove::Remove<'_>> for remove::remove_paths_t {
    fn from_new(new: features::remove::Remove) -> Self {
        Self {
            path: new.path.into_shape(),
            must_exist: new.must_exist,
        }
    }
}

impl FromNew<features::requires::Requires<'_>> for requires::requires_t {
    fn from_new(new: features::requires::Requires) -> Self {
        Self {
            files: Some(new.files.into_iter().map(IntoShape::into_shape).collect()),
            users: Some(new.users.into_iter().map(IntoShape::into_shape).collect()),
            groups: Some(new.groups.into_iter().map(IntoShape::into_shape).collect()),
        }
    }
}

impl FromNew<features::rpms::Action> for rpms::action_t {
    fn from_new(new: features::rpms::Action) -> Self {
        match new {
            features::rpms::Action::Install => Self::INSTALL,
            features::rpms::Action::RemoveIfExists => Self::REMOVE_IF_EXISTS,
        }
    }
}

impl FromNew<features::rpms::VersionSet<'_>> for rpms::version_set_t {
    fn from_new(new: features::rpms::VersionSet) -> Self {
        match new {
            features::rpms::VersionSet::Path(l) => {
                Self::String(l.to_str().expect("valid utf8").to_owned())
            }
            features::rpms::VersionSet::Source(m) => Self::Dict_String_To_String(
                m.into_iter()
                    .map(|(k, v)| (k.into_owned(), v.into_owned()))
                    .collect(),
            ),
        }
    }
}

impl FromNew<features::rpms::Rpm<'_>> for rpms::rpm_action_item_t {
    fn from_new(new: features::rpms::Rpm) -> Self {
        Self {
            name: match &new.source {
                features::rpms::Source::Name(name) => Some(name.clone().into_owned()),
                _ => None,
            },
            source: match new.source {
                features::rpms::Source::Source(p) => Some(p.into_shape()),
                features::rpms::Source::Name(_) => None,
            },
            action: new.action.into_shape(),
            flavor_to_version_set: new
                .flavor_to_version_set
                .into_iter()
                .map(|(k, v)| (k.name().to_owned(), v.into_shape()))
                .collect(),
        }
    }
}

impl FromNew<features::symlink::Symlink<'_>> for symlink::symlink_t {
    fn from_new(new: features::symlink::Symlink<'_>) -> Self {
        Self {
            dest: new.link.into_shape(),
            source: new.target.into_shape(),
        }
    }
}

impl FromNew<features::tarball::Tarball<'_>> for tarball::tarball_t {
    fn from_new(new: features::tarball::Tarball) -> Self {
        Self {
            into_dir: new.into_dir.into_shape(),
            source: new.src.into_shape(),
            force_root_ownership: Some(new.force_root_ownership),
        }
    }
}

impl FromNew<features::usergroup::User<'_>> for usergroup::user_t {
    fn from_new(new: features::usergroup::User) -> Self {
        Self {
            name: new.name.into_shape(),
            id: new.uid.map(|u| u.id().into()),
            primary_group: new.primary_group.into_shape(),
            supplementary_groups: new
                .supplementary_groups
                .into_iter()
                .map(|g| g.into_shape())
                .collect(),
            shell: new.shell.into_shape(),
            home_dir: new.home_dir.into_shape(),
            comment: new.comment.map(|c| c.into_owned()),
        }
    }
}

impl FromNew<features::usergroup::UserMod<'_>> for usergroup::usermod_t {
    fn from_new(new: features::usergroup::UserMod) -> Self {
        Self {
            username: new.username.into_shape(),
            add_supplementary_groups: new
                .add_supplementary_groups
                .into_iter()
                .map(|g| g.into_shape())
                .collect(),
        }
    }
}

impl FromNew<features::usergroup::Group<'_>> for usergroup::group_t {
    fn from_new(new: features::usergroup::Group) -> Self {
        Self {
            name: new.name.into_shape(),
            id: new.gid.map(|g| g.id().into()),
        }
    }
}
