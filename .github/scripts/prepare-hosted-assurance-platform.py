#!/usr/bin/env python3
"""Select public platform pins on disposable hosted jobs, before assurance freeze."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise SystemExit(reason)


def environment(name: str, value: str) -> None:
    require(not any(c in value for c in '\r\n'), 'unsafe environment value')
    with Path(os.environ['GITHUB_ENV']).open('a') as output:
        output.write(f'{name}={value}\n')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=('android', 'macos', 'ios'), required=True)
    args = parser.parse_args()
    require(os.environ.get('GITHUB_ACTIONS') == 'true'
            and os.environ.get('BOOTSTRAP_RUNNER_ENVIRONMENT') == 'github-hosted',
            'explicit GitHub-hosted setup context required')
    pins = json.loads(os.environ.get('OA18_PLATFORM_JSON') or '{}')
    receipt = {'profile': args.profile, 'pins': pins, 'commands': []}
    if args.profile == 'android':
        require(os.environ.get('RUNNER_OS') == 'Linux', 'Android preparation requires Linux')
        version = pins.get('android_ndk_version', '')
        require(isinstance(version, str) and re.fullmatch(r'\d+\.\d+\.\d+', version) is not None,
                'independently configured exact android_ndk_version required')
        expected = pins.get('java_version', '')
        require(isinstance(expected, str) and re.fullmatch(r'\d+\.\d+\.\d+(?:\.\d+)?', expected) is not None,
                'exact public java_version required')
        result = subprocess.run(['java', '-version'], text=True, capture_output=True, check=True)
        require('"' + expected + '"' in result.stderr + result.stdout,
                'preinstalled Java version differs from approved pin')
        receipt['java'] = result.stderr + result.stdout
        sdk = Path(os.environ['ANDROID_HOME']).resolve()
        argv = ['sdkmanager', '--install', f'ndk;{version}']
        subprocess.run(argv, check=True)
        receipt['commands'].append(argv)
        home = sdk / 'ndk' / version
        properties = (home / 'source.properties').read_text()
        require(re.search(r'^Pkg.Revision\s*=\s*' + re.escape(version) + r'\s*$', properties, re.M) is not None,
                'installed NDK identity differs from approved pin')
        environment('ANDROID_NDK_HOME', str(home))
        receipt['source_properties'] = properties
    else:
        require(os.environ.get('RUNNER_OS') == 'macOS', 'Apple preparation requires macOS')
        version = pins.get('xcode_version', '')
        build = pins.get('xcode_build', '')
        require(isinstance(version, str) and re.fullmatch(r'\d+\.\d+(?:\.\d+)?', version) is not None
                and isinstance(build, str) and re.fullmatch(r'[A-Za-z0-9]+', build) is not None,
                'independently configured exact xcode_version and xcode_build required')
        home = Path('/Applications') / f'Xcode_{version}.app' / 'Contents/Developer'
        require(home.is_dir(), 'approved Xcode version is not installed on hosted image')
        selected = dict(os.environ, DEVELOPER_DIR=str(home))
        result = subprocess.run(['xcodebuild', '-version'], env=selected, text=True, capture_output=True, check=True)
        require(result.stdout.strip() == f'Xcode {version}\nBuild version {build}', 'preinstalled Xcode build differs from approved pin')
        environment('DEVELOPER_DIR', str(home))
        receipt['xcode'] = result.stdout
    (Path(os.environ['RUNNER_TEMP']) / 'oa18-platform-preparation.json').write_text(json.dumps(receipt, indent=2) + '\n')


if __name__ == '__main__':
    main()
