// @ts-check

/** @type {import('@docusaurus/plugin-content-docs').SidebarsConfig} */
const sidebars = {
  docs: [
    "introduction",
    "installation",
    "getting-started",
    {
      type: "category",
      label: "Guide",
      collapsed: false,
      items: [
        "guide/tasks",
        "guide/steps",
        "guide/variables",
        "guide/incremental",
        "guide/services",
        "guide/cli",
      ],
    },
    {
      type: "category",
      label: "API",
      link: { type: "doc", id: "api/index" },
      collapsed: false,
      items: ["api/cmd", "api/fs", "api/net", "api/env"],
    },
    "examples",
    "sandbox",
  ],
};

module.exports = sidebars;
