# Import & Export (Backup)

You can easily move your data between computers or create secure backups.

## Exporting Data
1.  Click the **Settings** (Gear) icon in the sidebar.
2.  Click **Export Backup**.
3.  **Set a Password**: You MUST create a password for this backup file.
    -   This password is used to re-encrypt your data into the backup file.
    -   **Do not lose this password!** You cannot restore the backup without it.
4.  Save the `.json` file to a safe location.

## Importing Data
1.  Click the **Settings** (Gear) icon.
2.  Click **Import Backup**.
3.  Select your backup `.json` file.
4.  Enter the password you created when exporting.
5.  **Confirm**:
    > [!CAUTION]
    > Importing will **REPLACE** your current host list with the data from the backup.

## Troubleshooting
-   **"Invalid Password"**: Ensure you are using the password you set during *Export*, not your SSH password.
-   **"Corrupt File"**: Ensure the JSON file has not been modified manually.
