# @generated SignedSource<<64e9b568e1cb5cb3d7b01bdf1a514004>>
extension_rust_targets = [
    "//antlir:artifacts_dir_rs-rust",
    "//antlir:find_built_subvol_rs-rust",
    "//antlir:fs_utils_rs-rust",
    "//antlir:signed_source-rust",
    "//antlir/buck/buck_label:buck_label_py-rust",
    "//antlir/buck/targets_and_outputs:targets_and_outputs_py-rust",
    "//antlir/compiler/rust:compiler-rust",
    "//antlir/compiler/rust:mount-rust",
]
extension_modules = {
    "artifacts_dir_rs": "antlir.artifacts_dir_rs",
    "buck_label_py": "antlir.buck.buck_label.buck_label_py",
    "compiler": "antlir.compiler.rust.compiler",
    "find_built_subvol_rs": "antlir.find_built_subvol_rs",
    "fs_utils_rs": "antlir.fs_utils_rs",
    "mount": "antlir.compiler.rust.mount",
    "signed_source": "antlir.signed_source",
    "targets_and_outputs_py": "antlir.buck.targets_and_outputs.targets_and_outputs_py",
}
