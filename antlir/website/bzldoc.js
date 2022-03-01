/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

const util = require('util');
const exec = util.promisify(require('child_process').exec);
const {isInternal} = require('internaldocs-fb-helpers');
const args = require('minimist')(process.argv.slice(2))

const cp_cmd = ' && cp -r $(pwd)/docs/api/starlark/fbcode/antlir/* $(pwd)/docs/api';
const internal_cmd = 'buck run fbcode//buck2/buck2_docs:buck2_docs --  --destination-dir=$(pwd)/docs/api ';
const oss_cmd = 'buck run //generated/buck2/buck2_docs:buck2_docs --  --destination-dir=$(pwd)/docs/api '

// eslint-disable-next-line no-unused-vars
let bzldoc = (files_to_generate) => {
  if (isInternal()) {
    exec(
      internal_cmd + files_to_generate + cp_cmd,
      {
        shell: '/bin/bash',
      },
    );
  } else {
    exec(
      oss_cmd + files_to_generate + cp_cmd,
      {
        shell: '/bin/bash',
      },
    );
  }
};

bzldoc(args["files"]);
