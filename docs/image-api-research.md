# 图片检索 API 调研记录

调研日期：2026-11-13。仅查阅公开资料，未进行付费调用。

## 最终选择：豆包搜索

- 官方文档：https://www.volcengine.com/docs/87772/2272953
- 已安装的官方 Skill：`C:\Users\Victo\.agents\skills\byted-web-search`
- `SearchType=image`，`Count` 为 1–5；读取 `Result.ImageResults[].Image.Url/Width/Height`。
- 工作台使用 `WEB_SEARCH_API_KEY`，通过 API Key 直连接口。
- 图片搜索支持标题、图片地址、尺寸；来源页字段按实际返回的可选 `Url/SourceUrl/PageUrl` 读取，未断言服务一定返回来源页。
- 检索得到图片不代表拥有复制、修改或再发布授权，使用前需核查。

## 网页资料：DeepSeek Search / Fetch

- Search：https://api-docs.deepseek.com/zh-cn/api/search
- Fetch：https://api-docs.deepseek.com/zh-cn/api/fetch
- 使用指南：https://api-docs.deepseek.com/zh-cn/guides/search
- `POST /v1/tools/search` 返回网页证据，`POST /v1/tools/fetch` 返回标题与 Markdown 正文。
- 官方 API 文档未提供图片搜索参数或图片结果字段，工作台不将 `site_icon` 当作参考截图。
- 官方视觉输入文档：https://api-docs.deepseek.com/zh-cn/guides/vision
- `deepseek-flash` 支持 `image_url` 图像块；工作台默认使用它观察已下载原图后选择标注位置。

## 备选：SerpApi Google Images

- 接口：https://serpapi.com/google-images-api
- 返回字段：https://serpapi.com/images-results
- 价格：https://serpapi.com/pricing
- 返回原图、缩略图、来源网页、尺寸等；本次查询价格页列出每月 250 次免费搜索，之后按套餐收费，实际以官网为准。
- 结果未声明图片授权，不等于可直接使用。

## 不建议新接入：Bing Image Search

- 已归档官方页面：https://learn.microsoft.com/en-us/previous-versions/bing/search-apis/bing-image-search/overview
- 页面标记 `is_archived: true`、`is_retired: true`，因此不用于新项目。

## 其他候选

Brave Image Search 官方文档站本次网络连接多次超时，未完成契约核验，故不作接入承诺。Google Custom Search 官方页面也未能在本次网络环境中完成核验，不引用其未验证的额度或字段。
