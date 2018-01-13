const merge = require("webpack-merge");
const common = require("./webpack.common.js");
const UglifyJsPlugin = require('uglifyjs-webpack-plugin');

module.exports = module.exports = merge(common, {
  
  plugins: [
      new UglifyJsPlugin({
        sourceMap: true
      })
  ]
 
});