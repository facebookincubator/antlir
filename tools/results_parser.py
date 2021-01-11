#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Called from GitHub Actions to reformat the results from `buck test` into
# something more usable

import argparse
import enum
import sys
import xml.etree.ElementTree as ET
from dataclasses import dataclass

from blocklist import blocklist


parser = argparse.ArgumentParser()
parser.add_argument("xml")
parser.add_argument("--no-details", action="store_true")

args = parser.parse_args()

tree = ET.parse(args.xml)
root = tree.getroot()


class Status(enum.Enum):
    PASS = "PASS"
    FAIL = "FAIL"


@dataclass(frozen=True)
class Result(object):
    target: str
    name: str
    status: str
    message: str
    stacktrace: str

    @property
    def full_name(self) -> str:
        return f"{self.target} - {self.name}"


results = []

for test in root:
    target = test.attrib["name"]
    for result in test:
        results.append(
            Result(
                target=target,
                name=result.attrib["name"],
                status=Status(result.attrib["status"]),
                message=result.find("message").text,
                stacktrace=result.find("stacktrace").text,
            )
        )


# filter out tests that are disabled with the OSS blocklist
blocked_tests = [
    case
    for case in results
    if any(block.match(case.full_name) for block in blocklist)
]
results = [
    case
    for case in results
    if not any(block.match(case.full_name) for block in blocklist)
]

passed_count = sum(1 for case in results if case.status == Status.PASS)
failed_count = sum(1 for case in results if case.status == Status.FAIL)
print(f"{len(results)} total test cases")

print(f"\033[92mPASSED {passed_count} test cases:")
for case in results:
    if case.status != Status.PASS:
        continue
    print(f"\033[92m  {case.full_name}")

print("\033[0m")

print(f"{len(blocked_tests)} disabled tests still ran")
for case in blocked_tests:
    print(f"  {case.full_name} - {case.status}")

# print failures at the end since that is more visible on GitHub Actions
print(f"\033[91mFAIL {failed_count} test cases:")
for case in results:
    if case.status != Status.FAIL:
        continue
    print(f"\033[91m  {case.full_name}")
    if not args.no_details:
        details = (
            case.message.splitlines()
            + ["\n"]
            + case.stacktrace.splitlines()
            + ["\n\n"]
        )
        details = "\n".join(["\033[91m    " + line for line in details])
        print(details)

print("\033[0m")

if failed_count:
    sys.exit(1)
