const webpack = require('webpack');
const merge = require("webpack-merge");
const common = require("./webpack.common.js");
const UglifyJsPlugin = require('terser-webpack-plugin');

module.exports = merge(common, {
  mode: 'production',
  plugins: [
    new webpack.DefinePlugin({
      AUDIOSERVE_DEVELOPMENT: JSON.stringify(false)
    }),
    new UglifyJsPlugin({
      sourceMap: true
    })
  ]

});