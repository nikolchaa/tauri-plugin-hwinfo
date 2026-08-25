## Default Permission

Allows every hardware section to be read.

Scans run in safe mode unless the application explicitly opted in with
`Builder::new().allow_unsafe_scan(true)`, so this permission alone never
exposes serial numbers, MAC addresses or machine identifiers.

Grant the individual `allow-get-*-info` permissions instead if the frontend
only needs part of the inventory.


#### This default permission set includes the following:

- `allow-get-system-info`
- `allow-get-cpu-info`
- `allow-get-gpu-info`
- `allow-get-memory-info`
- `allow-get-storage-info`
- `allow-get-network-info`
- `allow-get-display-info`
- `allow-get-battery-info`
- `allow-get-board-info`
- `allow-get-os-info`

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`hwinfo:allow-get-battery-info`

</td>
<td>

Enables the get_battery_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:deny-get-battery-info`

</td>
<td>

Denies the get_battery_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:allow-get-board-info`

</td>
<td>

Enables the get_board_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:deny-get-board-info`

</td>
<td>

Denies the get_board_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:allow-get-cpu-info`

</td>
<td>

Enables the get_cpu_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:deny-get-cpu-info`

</td>
<td>

Denies the get_cpu_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:allow-get-display-info`

</td>
<td>

Enables the get_display_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:deny-get-display-info`

</td>
<td>

Denies the get_display_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:allow-get-gpu-info`

</td>
<td>

Enables the get_gpu_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:deny-get-gpu-info`

</td>
<td>

Denies the get_gpu_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:allow-get-memory-info`

</td>
<td>

Enables the get_memory_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:deny-get-memory-info`

</td>
<td>

Denies the get_memory_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:allow-get-network-info`

</td>
<td>

Enables the get_network_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:deny-get-network-info`

</td>
<td>

Denies the get_network_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:allow-get-os-info`

</td>
<td>

Enables the get_os_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:deny-get-os-info`

</td>
<td>

Denies the get_os_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:allow-get-storage-info`

</td>
<td>

Enables the get_storage_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:deny-get-storage-info`

</td>
<td>

Denies the get_storage_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:allow-get-system-info`

</td>
<td>

Enables the get_system_info command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`hwinfo:deny-get-system-info`

</td>
<td>

Denies the get_system_info command without any pre-configured scope.

</td>
</tr>
</table>
