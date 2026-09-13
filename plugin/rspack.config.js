// Figma 플러그인은 파일 두 개를 원한다. 메인 스레드용 `code.js` 하나와, UI
// iframe 에 그대로 인라인된 `ui.html` 하나다. iframe 은 외부 스크립트를 불러올
// 수 없어서 UI 번들은 HTML 안에 박혀야 한다.

import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import HtmlWebpackPlugin from 'html-webpack-plugin'

const here = dirname(fileURLToPath(import.meta.url))

/** @type {import('@rspack/core').Configuration[]} */
export default [
  {
    name: 'code',
    target: 'web',
    mode: 'production',
    // `dist/` 는 커밋된다. 소스맵은 Figma 가 읽지 않으면서 번들의 네 배가 넘고,
    // 빌드마다 저장소에 들어갈 이유가 없다.
    devtool: false,
    entry: { code: resolve(here, 'src/code.ts') },
    output: { path: resolve(here, 'dist'), filename: '[name].js' },
    resolve: { extensions: ['.ts', '.js'] },
    module: {
      rules: [
        {
          test: /\.ts$/,
          use: {
            loader: 'builtin:swc-loader',
            options: {
              jsc: { parser: { syntax: 'typescript' }, target: 'es2020' },
            },
          },
        },
      ],
    },
    // 플러그인 샌드박스에는 청크 로더가 없다. 한 파일로 나와야 한다.
    optimization: { splitChunks: false },
  },
  {
    name: 'ui',
    target: 'web',
    mode: 'production',
    devtool: false,
    entry: { ui: resolve(here, 'src/ui.ts') },
    output: { path: resolve(here, 'dist'), filename: '[name].js' },
    resolve: { extensions: ['.ts', '.js'] },
    module: {
      rules: [
        {
          test: /\.ts$/,
          use: {
            loader: 'builtin:swc-loader',
            options: {
              jsc: { parser: { syntax: 'typescript' }, target: 'es2020' },
            },
          },
        },
      ],
    },
    plugins: [
      new HtmlWebpackPlugin({
        template: resolve(here, 'src/ui.html'),
        filename: 'ui.html',
        chunks: ['ui'],
        inject: 'body',
        // 번들 인라인은 빌드 뒤 scripts/inline-ui.mjs 가 처리한다.
        scriptLoading: 'blocking',
      }),
    ],
    optimization: { splitChunks: false },
  },
]
