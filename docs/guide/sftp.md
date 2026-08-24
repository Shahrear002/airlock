# SFTP File Transfer

Airlock features a built-in SFTP client that allows you to manage files on your remote servers alongside your terminal sessions.

## Opening the SFTP Browser

To access the SFTP browser:
1.  Connect to a host.
2.  Click the **Files** icon (or similar) in the sidebar or use the tab-specific view if available.
3.  The SFTP browser will open in a dual-pane layout.

## Dual-Pane Layout

The SFTP browser is divided into two main sections:
-   **Local Pane**: Displays the files and folders on your local machine.
-   **Remote Pane**: Displays the files and folders on the connected remote server.

Each pane supports full navigation, allowing you to browse through directory trees.

## Transferring Files

Airlock makes file transfers intuitive through drag-and-drop:

### Uploading Files
1.  Locate the file or folder in the **Local Pane**.
2.  **Drag** it and **drop** it into the **Remote Pane**.
3.  The transfer will begin automatically.

### Downloading Files
1.  Locate the file or folder in the **Remote Pane**.
2.  **Drag** it and **drop** it into the **Local Pane**.
3.  The file will be downloaded to the selected local directory.

## Recursive Transfers

Airlock supports recursive transfers for folders. When you drag a folder, all its contents, including subdirectories, will be transferred while maintaining the original structure.

## Transfer Progress & Cancellation

You can monitor active transfers in real-time:
-   **Progress Bar**: Shows the current status of each file transfer.
-   **Cancellation**: If you need to stop a transfer, you can click the **Cancel** button associated with the active task. This will safely halt the process.

## Editing Files Locally (New in v1.2.2)

Airlock allows you to seamlessly edit remote files using your favorite local editor (VS Code, Notepad, etc.):
1. **Right-click** any file in the **Remote Pane**.
2. Select **Edit Locally**.
3. Airlock will securely download a temporary copy and open it in your OS's default editor.
4. **Save** the file in your editor, and Airlock will automatically detect the change and upload it back to the server in the background.

> [!TIP]
> The temporary files are securely isolated and automatically managed by Airlock, so you don't have to worry about cleaning them up.

## Navigation Controls

-   **Double-click**: Open a folder to view its contents.
-   **Breadcrumbs**: Use the breadcrumb path at the top of each pane to quickly navigate back to parent directories.
-   **Refresh**: Use the refresh icon to reload the current directory listing.
