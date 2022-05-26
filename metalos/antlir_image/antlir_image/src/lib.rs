pub mod layer;
pub mod partition;
pub mod path;
pub mod subvolume;

/// This is a marker trait to indicidate that this struct can be the top level
/// object inside of an atlir package. Should really be just a GPT image or an
/// antlir layer
pub trait AntlirPackaged {}

#[cfg(test)]
mod test {
    use std::path::{Path, PathBuf};

    use maplit::hashmap;

    use crate::partition::AntlirGPT;
    use crate::path::VerifiedPath;
    use crate::subvolume::AntlirSubvolume;
    use crate::{generate_layer, generate_partitions, generate_paths, generate_subvolumes};

    generate_layer! {
        ControlLayer {
            paths: ControlPaths,
        }
    }

    generate_paths! {
        ControlPaths {
            image_initrd(VerifiedPath, "image/initrd"),
            image_kernel(VerifiedPath, "image/kernel"),
            image_rootfs(VerifiedPath, "image/rootfs"),
            image_service(VerifiedPath, "image/service"),
            image_service_config_generator(VerifiedPath, "image/service-config-generator"),
            run_boot(VerifiedPath, "run/boot"),
            run_cache(VerifiedPath, "run/cache"),
            run_logs(VerifiedPath, "run/logs"),
            run_runtime(VerifiedPath, "run/runtime"),
            run_scratch(VerifiedPath, "run/scratch"),
            run_service_roots(VerifiedPath, "run/service-roots"),
            run_state(VerifiedPath, "run/state"),
            run_state_metalos(VerifiedPath, "run/state/metalos"),
        }
    }

    generate_subvolumes! {
        RootDiskSubvolumes {
            volume(AntlirSubvolume<ControlLayer>, "/volume"),
        }
    }

    generate_partitions! {
        RootDiskGPT {
            p2(RootDiskSubvolumes, 2),
        }
    }

    #[test]
    fn test_basic() {
        let mount_point = PathBuf::from("/test/mount");
        let table = hashmap! {
            1 => VerifiedPath::new_unchecked("/dev/sda1".into()),
            2 => VerifiedPath::new_unchecked("/dev/sda2".into()),
        };

        let gpt = RootDiskGPT::from_partition_map_unchecked(
            VerifiedPath::new_unchecked("/dev/sda".into()),
            table,
        )
        .expect("We provided all the needed partitions");
        assert_eq!(gpt.disk, VerifiedPath::new_unchecked("/dev/sda".into()));
        let partition = gpt.p2;

        assert_eq!(partition.number, 2);
        assert_eq!(
            partition.path,
            VerifiedPath::new_unchecked("/dev/sda2".into())
        );

        let volume = partition.subvolumes.volume();
        assert_eq!(&volume.relative_path, Path::new("/volume"));

        let layer = volume.mount_unchecked(VerifiedPath::new_unchecked(mount_point));
        let paths = layer.paths;

        assert_eq!(
            paths.image_initrd().path(),
            Path::new("/test/mount/image/initrd")
        );
        assert_eq!(
            paths.run_state_metalos().path(),
            Path::new("/test/mount/run/state/metalos")
        );
    }
}
