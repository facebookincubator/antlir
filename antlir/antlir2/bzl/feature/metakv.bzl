# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

types.lint_noop()

def metakv_store(
        *,
        key: types.or_selector(str.type),
        value: types.or_selector(str.type),
        require_keys: types.or_selector([types.or_selector(str.type)]) = [],
        store_if_not_exists: types.or_selector(bool.type) = False) -> ParseTimeFeature.type:
    """
    `metakv_store("key", "value")` writes the key value pair into
    the META_KEY_VALUE_STORE_FILE in the image. This can be read later. It is enforced that
    for every unique key there can be only one corresponding value set.

    The arguments `key` and `value` are mandatory; `provides`, and `require` are optional.

    The argument `key` is the key to be written, and `value` is the value written.

    The argument `require_keys` defines a list of requirements to be satisfied before the current value
    and is used for ordering individual key value pairs. For example, we might want to store
    a list of pre_run_commands, which must be run in a specific order. Since Antlir features
    are unordered, we need provides/requires semantics to order the individual key/value metadata
    pairs that form this list. Note that we can't just pass the array all in as a single item
    because child layers might want to install their own pre-run-commands.

    The argument `store_if_not_exists` only adds the value if the key doesn't exist. If the key
    exists, this is a no-op.
    """
    return ParseTimeFeature(
        feature_type = "metakv",
        kwargs = {
            "store": {
                "key": key,
                "require_keys": require_keys,
                "store_if_not_exists": store_if_not_exists,
                "value": value,
            },
        },
    )

def metakv_remove(*, key: types.or_selector(str.type)):
    """
    `metakv_remove("key")` removes the key value pair that was written into the
    META_KEY_VALUE_STORE_FILE in the image. This throws an error if the key is
    not present.

    The argument `key` is the value to remove.
    """
    return ParseTimeFeature(
        feature_type = "metakv",
        kwargs = {
            "remove": {
                "key": key,
            },
        },
    )

metakv_store_record = record(
    key = str.type,
    value = str.type,
    require_keys = [str.type],
    store_if_not_exists = bool.type,
)

metakv_remove_record = record(
    key = str.type,
)

metakv_record = record(
    store = [metakv_store_record.type, None],
    remove = [metakv_remove_record.type, None],
)

def metakv_analyze(
        store: [{str.type: ""}, None] = None,
        remove: [{str.type: ""}, None] = None) -> FeatureAnalysis.type:
    return FeatureAnalysis(
        data = metakv_record(
            store = metakv_store_record(**store) if store else None,
            remove = metakv_remove_record(**remove) if remove else None,
        ),
    )
