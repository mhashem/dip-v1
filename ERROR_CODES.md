# DIP-DB Error Codes

This document lists all the error codes returned by the DIP-DB engine, along with their meanings and possible resolutions.

## 1xxx: SQL Parse Errors
*   **1001**: `Syntax Error` - The SQL statement could not be parsed. Check your syntax.
*   **1002**: `Unsupported Statement` - The statement type (e.g., `DROP TABLE`) is not yet supported.

## 2xxx: Catalog Errors
*   **2001**: `Table Not Found` - The specified table does not exist.
*   **2002**: `Table Already Exists` - Attempted to create a table with a name that is already in use.
*   **2003**: `Column Not Found` - The specified column does not exist in the table schema.

## 3xxx: Execution & Data Errors
*   **3001**: `Type Mismatch` - Attempted to insert or compare incompatible types (e.g., Integer vs Boolean).
*   **3002**: `Primary Key Constraint Violation` - Attempted to insert a duplicate value into a Primary Key column.
*   **3003**: `Unique Constraint Violation` - Attempted to insert a duplicate value into a Unique Index.
*   **3004**: `Transaction Aborted` - The transaction was aborted due to a conflict or deadlock.

## 4xxx: Storage Errors
*   **4001**: `Page Read Error` - Failed to read a page from disk.
*   **4002**: `Page Write Error` - Failed to write a page to disk.
*   **4003**: `Buffer Pool Full` - No free frames available in the buffer pool (and eviction failed).
