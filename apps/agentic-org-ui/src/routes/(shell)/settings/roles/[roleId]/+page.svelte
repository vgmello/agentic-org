<script lang="ts">
	import { page } from '$app/state';
	import RolePage from '$lib/modules/roles/features/role-page/RolePage.svelte';

	let { data } = $props();
</script>

<!-- Another role or organization is another page: its read starts again. The page sets its own title. -->
{#key `${data.connection.organization.id}/${page.params.roleId}`}<RolePage
		organizationId={data.connection.organization.id}
		roleId={page.params.roleId ?? ''}
	/>{/key}
