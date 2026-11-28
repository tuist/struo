---
title: Hosted tests can import apps with custom product names
date: 2026-11-27
---

Hosted unit tests can now `@testable import` their app when its Swift module
name differs from the product name used for the bundle. For example, a product
named `Repro App` is still built as `Repro App.app`, and its tests can import
`Repro_App`.
