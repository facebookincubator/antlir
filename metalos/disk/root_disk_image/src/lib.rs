use antlir_image::generate_layer;
use antlir_image::generate_partitions;
use antlir_image::generate_paths;
use antlir_image::generate_subvolumes;
use antlir_image::path::VerifiedPath;
use antlir_image::subvolume::AntlirSubvolume;

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
        run_kernel(VerifiedPath, "run/kernel"),
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
