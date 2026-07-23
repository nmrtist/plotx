// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
	site: 'https://docs.plotx.nmrtist.space',
	integrations: [
		starlight({
			title: 'PlotX',
			description: 'User manual for PlotX, a desktop application for scientific data analysis and figure preparation.',
			logo: {
				light: './src/assets/logo-light.svg',
				dark: './src/assets/logo-dark.svg',
			},
			defaultLocale: 'root',
			locales: {
				root: { label: 'English', lang: 'en' },
				'zh-cn': { label: '简体中文', lang: 'zh-CN' },
			},
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/nmrtist/plotx' }],
			sidebar: [
				{
					label: 'Getting Started',
					translations: { 'zh-CN': '快速上手' },
					items: [
						{ slug: 'getting-started/installation' },
						{ slug: 'getting-started/quick-tour' },
						{ slug: 'getting-started/first-figure' },
					],
				},
				{
					label: 'Working with Data',
					translations: { 'zh-CN': '准备数据' },
					items: [
						{ slug: 'guides/importing-data' },
						{ slug: 'guides/organizing-data' },
						{ slug: 'guides/tables' },
						{ slug: 'guides/processing' },
					],
				},
				{
					label: 'Analysis',
					translations: { 'zh-CN': '分析' },
					items: [
						{ slug: 'guides/choosing-a-tool' },
						{ slug: 'guides/peaks-and-regions' },
						{ slug: 'guides/curve-fitting' },
						{ slug: 'guides/custom-models' },
						{ slug: 'guides/statistics' },
						{ slug: 'guides/spectrum-tools' },
						{
							label: 'By technique',
							translations: { 'zh-CN': '按数据类型' },
							items: [
								{ slug: 'guides/pseudo-2d' },
								{ slug: 'guides/2d-integration' },
								{ slug: 'guides/electrophysiology' },
							],
						},
					],
				},
				{
					label: 'Figures and Export',
					translations: { 'zh-CN': '图形与导出' },
					items: [
						{ slug: 'guides/layout-and-export' },
						{ slug: 'guides/annotations' },
						{ slug: 'guides/exporting' },
						{ slug: 'guides/present-mode' },
					],
				},
				{
					label: 'Batch and Automation',
					translations: { 'zh-CN': '批量与自动化' },
					items: [
						{ slug: 'guides/automation' },
						{ slug: 'guides/templates' },
						{ slug: 'reference/cli' },
					],
				},
				{
					label: 'Reference',
					translations: { 'zh-CN': '参考' },
					items: [
						{ slug: 'reference/shortcuts' },
						{ slug: 'reference/command-palette' },
						{ slug: 'reference/ui-overview' },
						{ slug: 'reference/preferences' },
						{ slug: 'reference/file-formats' },
						{ slug: 'reference/updates' },
						{ slug: 'reference/troubleshooting' },
						{ slug: 'reference/reporting-problems' },
					],
				},
			],
		}),
	],
});
