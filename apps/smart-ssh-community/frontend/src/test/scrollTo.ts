// jsdom has no `Element.prototype.scrollTo` (issue #906).
if (!Element.prototype.scrollTo) {
  Element.prototype.scrollTo = () => {};
}
