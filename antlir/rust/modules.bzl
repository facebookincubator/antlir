# @generated SignedSource<<1ab0c378cceadc779a229bbf087971f8>>
extension_rust_targets = [
    "//antlir/buck/targets_and_outputs:targets_and_outputs_py-rust",
    "//antlir/compiler/rust:compiler-rust",
    "//antlir:artifacts_dir_rs-rust",
    "//antlir:find_built_subvol_rs-rust",
    "//antlir:fs_utils_rs-rust",
    "//antlir:signed_source-rust",
]
extension_modules = {
    "artifacts_dir_rs": "antlir.artifacts_dir_rs",
    "compiler": "antlir.compiler.rust.compiler",
    "find_built_subvol_rs": "antlir.find_built_subvol_rs",
    "fs_utils_rs": "antlir.fs_utils_rs",
    "signed_source": "antlir.signed_source",
    "targets_and_outputs_py": "antlir.buck.targets_and_outputs.targets_and_outputs_py",
}