const webpack = require('webpack');
const { merge } = require("webpack-merge");
const common = require("./webpack.common.js");
const TerserPlugin = require("terser-webpack-plugin");
const { map } = require('jquery');

module.exports = merge(common, {
  // devtool: 'source-map',
  mode: 'production',
  optimization: {
    minimize: true,
    minimizer: [new TerserPlugin()],
  },
  plugins: [
    new webpack.DefinePlugin({
      AUDIOSERVE_DEVELOPMENT: JSON.stringify(false)
    })
  ]

});