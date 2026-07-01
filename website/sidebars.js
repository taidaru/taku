// @ts-check

/** @type {import('@docusaurus/plugin-content-docs').SidebarsConfig} */
const sidebars = {
  docs: [
    "introduction",
    "installation",
    "getting-started",
    "tasks",
    {
      type: "category",
      label: "API",
      link: { type: "doc", id: "api/index" },
      collapsed: false,
      items: ["api/sh", "api/fs", "api/net", "api/env", "api/ssh"],
    },
    "examples",
    "sandbox",
  ],
};

module.exports = sidebars;
