/**
 * Copyright (c) Facebook, Inc. and its affiliates.
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

// eslint-disable-next-line no-unused-vars
module.exports = (context, options) => {
  const bzlDir = path.resolve(context.siteDir, '../bzl');
  return {
    name: 'bzldoc',
    async loadContent() {
      const out = `${context.siteDir}/docs/api/`;
      await exec(
        `shopt -s globstar && buck run //antlir/website/gen:bzldoc -- ${bzlDir}/**/*.bzl ${out}`, {
          shell: "/bin/bash",
        }
      );
      return null;
    },
    getPathsToWatch() {
      return bzlGlobs;
    },
  };
};
