/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

// Tiny docusaurus plugin to rerun bzldoc to regenerate the .md from .bzl
// docstrings on file changes during `yarn start`, or on `yarn build`

const path = require('path');
const util = require('util');
const exec = util.promisify(require('child_process').exec);
const {isInternal} = require('internaldocs-fb-helpers');

const cp_cmd = ' && cp -r $(pwd)/docs/api/starlark/fbcode/antlir/* $(pwd)/docs/api';
const cmd = 'buck2 docs starlark --format markdown_files --markdown-files-destination-dir=$(pwd)/docs/api ';
const files_to_generate = '//antlir/bzl:image.bzl //antlir/bzl:shape.bzl //antlir/bzl/image/feature:defs.bzl //antlir/bzl/image/package:defs.bzl //antlir/vm/bzl:defs.bzl //antlir/bzl:flavor_helpers.bzl //antlir/bzl:test_rpms.bzl';

// eslint-disable-next-line no-unused-vars
module.exports = (context, options) => {
  const bzlDir = path.resolve(context.siteDir, '../bzl');
  return {
    name: 'bzldoc',
    async loadContent() {
      const out = `${context.siteDir}/docs/api/`;
        await exec(
        cmd + files_to_generate + cp_cmd,
          {
            shell: '/bin/bash',
          },
        );
      return null;
    },
    getPathsToWatch() {
      return [`${bzlDir}/**/*.bzl`];
    },
  };
};
