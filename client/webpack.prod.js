const webpack = require('webpack');
const merge = require("webpack-merge");
const common = require("./webpack.common.js");
const UglifyJsPlugin = require('uglifyjs-webpack-plugin');

module.exports = module.exports = merge(common, {

  plugins: [
    new webpack.DefinePlugin({
      AUDIOSERVE_DEVELOPMENT: JSON.stringify(false)
    }),
    new UglifyJsPlugin({
      sourceMap: true
    })
  ]

});