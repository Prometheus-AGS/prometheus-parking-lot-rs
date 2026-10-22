import Link from '@docusaurus/Link';
import useBaseUrl from '@docusaurus/useBaseUrl';
import Layout from '@theme/Layout';
import styles from './index.module.css';

const sections = [
  {
    to: '/docs/intro',
    label: 'Overview',
    status: 'available',
    body: 'What the library is for, what this site covers, and how the documentation is being built.',
  },
  {
    to: '/docs/concepts',
    label: 'Concepts',
    status: 'coming in cr-46',
    body: 'Parking lots, admission control, leases, mailboxes and actors, worker pools, event logs, and why agentic workloads need them.',
  },
  {
    to: '/docs/guides',
    label: 'Guides',
    status: 'coming in cr-47',
    body: 'Deployment scenarios, configuration, and a link to the generated API reference.',
  },
];

const goals = [
  'Weighted capacity is never exceeded.',
  'Accepted work is never silently lost.',
  'Results reach clients that disconnected.',
];

function SectionCard({to, label, status, body}) {
  const planned = status !== 'available';
  return (
    <Link className={`pl-card ${styles.card}`} to={to}>
      <span className={`pl-pill ${planned ? 'pl-pill--planned' : ''}`}>{status}</span>
      <h3 className={styles.cardTitle}>{label}</h3>
      <p className={styles.cardBody}>{body}</p>
    </Link>
  );
}

export default function Home() {
  const logo = useBaseUrl('/img/logo.svg');
  return (
    <Layout
      title="Documentation"
      description="Documentation for Prometheus Parking Lot, a resource-aware parking-lot scheduler for Rust."
    >
      <main className={styles.main}>
        <section className={styles.hero} aria-labelledby="home-title">
          <img className={styles.mark} src={logo} alt="" width="88" height="88" />
          <div>
            <p className="pl-meta">prometheus_parking_lot · Rust · MIT</p>
            <h1 id="home-title" className={styles.title}>
              Prometheus Parking Lot
            </h1>
            <p className={styles.lede}>
              A resource-aware scheduler that admits work while capacity allows, parks the rest, and keeps
              results for clients that come back later.
            </p>
            <div className={styles.actions}>
              <Link className="button button--primary button--lg" to="/docs/intro">
                Read the overview
              </Link>
              <Link
                className="button button--secondary button--lg"
                href="https://github.com/Prometheus-AGS/prometheus-parking-lot-rs"
              >
                Source on GitHub
              </Link>
            </div>
          </div>
        </section>

        <section className={styles.sections} aria-labelledby="sections-title">
          <h2 id="sections-title" className={styles.sectionTitle}>
            Documentation
          </h2>
          <div className={styles.grid}>
            {sections.map((section) => (
              <SectionCard key={section.label} {...section} />
            ))}
          </div>
        </section>

        <section className={styles.goals} aria-labelledby="goals-title">
          <div>
            <h2 id="goals-title" className={styles.sectionTitle}>
              Current phase goals
            </h2>
            <p className={styles.goalsNote}>
              The correctness-and-durability phase is in progress. These are goals, not yet delivered
              guarantees; the changelog records what has shipped.
            </p>
          </div>
          <ol className={styles.goalList}>
            {goals.map((goal) => (
              <li key={goal}>{goal}</li>
            ))}
          </ol>
        </section>
      </main>
    </Layout>
  );
}
