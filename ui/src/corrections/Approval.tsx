import { useId } from 'react';
import { Input, Radio } from '../kit';
import { PIN_DIGITS } from '../auth/keyboard';

/** The same manager input for a void, a refund and the final corrected bill. */
export function ApprovalFields({ people, selected, pin, onSelect, onPin, disabled = false, pinLabel = 'Their PIN' }: {
  people: readonly { id: string; name: string }[];
  selected: string;
  pin: string;
  onSelect: (id: string) => void;
  onPin: (pin: string) => void;
  disabled?: boolean;
  pinLabel?: string;
}) {
  const name = useId();
  return <div className="mb-approval">
    <p className="mb-muted">This needs a manager. They enter their own PIN for this action.</p>
    {people.length === 0 ? <p role="alert">No eligible manager is available. Ask the owner to check staff permissions.</p> : null}
    <div className="mb-reasons">
      {people.map((person) => <Radio key={person.id} name={name} label={person.name}
        checked={selected === person.id} disabled={disabled} onChange={() => onSelect(person.id)} />)}
    </div>
    <Input label={pinLabel} type="password" inputMode="numeric" maxLength={PIN_DIGITS}
      value={pin} disabled={disabled} onChange={(event) => onPin(event.target.value.replace(/[^0-9]/g, ''))} />
  </div>;
}
