/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 *
 * @format
 */

import React from 'react';
import clsx from 'clsx';
import Layout from '@theme/Layout';
import Link from '@docusaurus/Link';
import useDocusaurusContext from '@docusaurus/useDocusaurusContext';
import useBaseUrl from '@docusaurus/useBaseUrl';
import styles from './styles.module.css';

const features = [
  {
    title: <>
      What you <a href="https://buck.build/"><tt>buck build</tt></a>...
    </>,
    imageUrl: 'img/undraw_docusaurus_mountain.svg',
    description: <ul>
      <li>Fast</li>
      <li>Safe, declarative API</li>
      <li>Source-repo determinstic</li>
      <li>RPM package support</li>
      <li>Pre-built artifact support</li>
    </ul>,
  },
  {
    title: <>is what you `buck test`...</>,
    imageUrl: 'img/undraw_docusaurus_tree.svg',
    description: <ul>
      <li>Run tests inside or outside your image</li>
      <li>Easily compare entire filesystems</li>
    </ul>,
  },
  {
    title: <>is what you deploy.</>,
    imageUrl: 'img/undraw_docusaurus_react.svg',
    description: <ul>
      <li>To hosts</li>
      <li>To multiple container runtimes</li>
      <li>In various packaging formats</li>
    </ul>,
  },
];

function Feature({imageUrl, title, description}) {
  const imgUrl = useBaseUrl(imageUrl);
  return (
    <div className={clsx('col col--4', styles.feature)}>
      {imgUrl && (
        <div className="text--center">
          <img className={styles.featureImage} src={imgUrl} alt={title} />
        </div>
      )}
      <h3>{title}</h3>
      <p>{description}</p>
    </div>
  );
}

function Home() {
  const context = useDocusaurusContext();
  const {siteConfig = {}} = context;
  return (
    <Layout
      title={`${siteConfig.title}`}
      description="A filesystem image builder">
      <header className={clsx('hero hero--primary', styles.heroBanner)}>
        <div className="container">
          {/* Left */}
          <div className={styles.heroLeft}>
            <div className={styles.imageLogo}>
              <img src="img/logo.svg" alt="antlers" />
            </div>
          </div>
          {/* Right */}
          <div className={styles.heroRight}>
            <h1 className="hero__title">{siteConfig.title}</h1>
            <p className="hero__subtitle">{siteConfig.tagline}</p>
            <div className={styles.buttons}>
              <Link
                className={clsx(
                  'button button--outline button--secondary button--lg',
                  styles.getStartedButton,
                )}
                to={useBaseUrl('docs/')}>
                Read the docs
              </Link>
            </div>
          </div>
        </div>
      </header>
      <main>
        {features && features.length > 0 && (
          <section className={styles.features}>
            <div className="container">
              <div className="row">
                {features.map(({title, imageUrl, description}) => (
                  <Feature
                    key={title}
                    title={title}
                    imageUrl={imageUrl}
                    description={description}
                  />
                ))}
              </div>
            </div>
          </section>
        )}
      </main>
    </Layout>
  );
}

export default Home;
