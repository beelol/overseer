// Metro bundles the app. The packages next to it (core/, model/, integration/) are imported as
// source; their own node_modules hold test tools only and are left out of the bundle's search.
const { getDefaultConfig } = require('expo/metro-config');

const config = getDefaultConfig(__dirname);
const escape = (path) => path.replace(/[/\\]/g, '[/\\\\]');
config.resolver.blockList = [
  new RegExp(`^${escape(__dirname)}[/\\\\](core|model|integration)[/\\\\]node_modules[/\\\\].*`),
  new RegExp(`^${escape(__dirname)}[/\\\\](core|model|integration)[/\\\\](test|scripts)[/\\\\].*`),
];
module.exports = config;
