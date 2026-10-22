// @ts-check
// Explicit sidebar so section order is reviewed, not inferred from file names.

/** @type {import('@docusaurus/plugin-content-docs').SidebarsConfig} */
const sidebars = {
  docs: [
    'intro',
    {
      type: 'category',
      label: 'Concepts',
      link: {type: 'doc', id: 'concepts/index'},
      items: [
        'concepts/parking-lots',
        'concepts/admission-control',
        'concepts/leases',
        'concepts/mailboxes-and-actors',
        'concepts/worker-pools',
        'concepts/event-logs',
        'concepts/agentic-value',
      ],
    },
    {
      type: 'category',
      label: 'Guides',
      link: {type: 'doc', id: 'guides/index'},
      items: ['guides/scenarios', 'guides/configuration', 'guides/api-reference'],
    },
    {
      type: 'category',
      label: 'Contributing',
      items: ['contributing/docs-site'],
    },
  ],
};

export default sidebars;
