/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

import React from 'react';
import clsx from 'clsx';
import styles from './HomepageFeatures.module.css';

const FeatureList = [
  {
    title: (<code>buck build</code>),
    Svg: require('../../static/img/logo.svg').default,
    description: (
      <ul>
        <li>Safe, declarative FS construction</li>
        <li>SCM-determistic builds</li>
        <li><code>buck build</code> your code</li>
        <li>Install upstream packages</li>
      </ul>
    ),
  },
  {
    title: (<code>buck test</code>),
    Svg: require('../../static/img/logo.svg').default,
    description: (
      <ul>
        <li>Run tests inside containers</li>
        <li>Run tests inside VMs</li>
        <li>Inspect whole filesystems</li>
      </ul>
    ),
  },
  {
    title: 'Deploy',
    Svg: require('../../static/img/logo.svg').default,
    description: (
      <ul>
        <li>Disk images to physical hosts</li>
        <li>Container images</li>
        <li>Many other packaging formats</li>
      </ul>
    ),
  },
];

function Feature({Svg, title, description}) {
  return (
    <div className={clsx('col col--4')}>
      <div className="text--center">
        <Svg className={styles.featureSvg} alt={title} />
      </div>
      <div className="padding-horiz--md">
        <h3>{title}</h3>
        <p>{description}</p>
      </div>
    </div>
  );
}

export default function HomepageFeatures() {
  return (
    <section className={styles.features}>
      <div className="container">
        <div className="row">
          {FeatureList.map((props, idx) => (
            <Feature key={idx} {...props} />
          ))}
        </div>
      </div>
    </section>
  );
}
