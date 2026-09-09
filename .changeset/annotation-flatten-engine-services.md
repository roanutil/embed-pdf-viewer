---
'@embedpdf/engine-services': minor
---

New `annotations.flatten` and `annotations.exportAppearance` worker jobs over the runtime's set-based flatten: resolve refs on the page, hand the set to one candidate plan, and either paint in place (bumping that page's revision and weak-annotation state like a page flatten) or into a scratch document returned as PDF bytes.
