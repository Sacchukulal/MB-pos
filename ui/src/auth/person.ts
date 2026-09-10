/** One person, as the Staff screen and the first run send them to `save_staff_member`. */

import type { StaffDetailView } from '../ipc/generated/StaffDetailView';
import type { StaffEdit } from '../ipc/generated/StaffEdit';

/** Somebody new: working here, full time, nothing else known yet. */
export function blankPerson(id: string): StaffEdit {
  return {
    id,
    name: '',
    roleId: null,
    status: 'active',
    pin: '',
    phone: '',
    designation: '',
    department: '',
    employmentType: 'full_time',
    address: '',
    emergencyName: '',
    emergencyPhone: '',
    idProof: '',
    leftOn: '',
  };
}

/** What the edit dialog starts from: the record as it is, with no new PIN. */
export function editOf(person: StaffDetailView): StaffEdit {
  return {
    id: person.id,
    name: person.name,
    roleId: person.roleId,
    status: person.status,
    pin: '',
    phone: person.phone,
    designation: person.designation,
    department: person.department,
    employmentType: person.employmentType,
    address: person.address,
    emergencyName: person.emergencyName,
    emergencyPhone: person.emergencyPhone,
    idProof: person.idProof,
    leftOn: person.leftOn,
  };
}
